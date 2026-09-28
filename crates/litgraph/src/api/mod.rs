// SPDX-License-Identifier: GPL-3.0-or-later
//! The agent-facing surface: one JSON request in, one JSON response out.
//!
//! ```
//! use litgraph::api::{handle, Catalog, Request};
//! let catalog = Catalog::embedded().unwrap();
//! let req: Request = serde_json::from_str(
//!     r#"{ "packs": ["cofc"], "scenario": { "params": { "rate": 900 } }, "op": { "op": "solve" } }"#,
//! ).unwrap();
//! let resp = handle(&req, &catalog);
//! assert!(resp.ok);
//! assert!(resp.result.unwrap()["value"].is_number());
//! ```
//!
//! Every response carries `warnings` (what the engine had to assume) and
//! `provenance` (packs, fingerprints, parameters, modeling modes, engine), so
//! an agent can tell authored facts from fallbacks without reading source.

mod catalog;
mod decide;
mod describe;
mod explore;
mod op;
mod render;
mod validate;

pub use catalog::{fingerprint, Catalog};
pub use describe::{describe, schema, SCHEMA_KINDS};
pub use op::{Op, StructureWhat};
use op::ViewOp;
pub use render::choice_label;
pub use validate::{detect, validate, Validation};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::algo::sim::SimOptions;
use crate::error::{Error, Result};
use crate::lint;
use crate::metrics;
use crate::model::{merge_patch, CompileOptions, Graph, LinkFile, Pack};
use crate::scenario::{Scenario, View};

/// Engine name and version.
pub const ENGINE: &str = concat!("litgraph ", env!("CARGO_PKG_VERSION"));
/// Version of the request/response contract. Bumped on breaking changes.
pub const API_VERSION: u32 = 1;

fn yes() -> bool {
    true
}

/// A request: which packs, which scenario, which operation.
#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Pack ids, forum keys, file stems, or paths. Empty = every pack.
    #[serde(default)]
    pub packs: Vec<String>,
    /// Apply `links.json` between loaded packs.
    #[serde(default = "yes")]
    pub links: bool,
    /// Keep v1 absorbing-terminal semantics (no `accept` continuations).
    #[serde(default)]
    pub no_continuations: bool,
    /// The matter.
    #[serde(default)]
    pub scenario: Scenario,
    /// The operation (default `describe`).
    #[serde(default)]
    pub op: Op,
}

/// A structured error.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ApiError {
    /// Stable code: parse, invalid, not-found, expr, numeric, io.
    pub code: String,
    /// Message.
    pub message: String,
    /// What to try next.
    pub hint: String,
}

/// All warnings with one code, collapsed.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct GroupedWarning {
    /// Stable code (`probability-fill`, `mixed-node`, `payoff-not-authored`, ...).
    pub code: String,
    /// Occurrences.
    pub count: usize,
    /// A representative message.
    pub example: String,
    /// Up to eight locations.
    pub at: Vec<String>,
}

/// The response envelope.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Response {
    /// Success.
    pub ok: bool,
    /// Request/response contract version.
    pub api_version: u32,
    /// The op that ran.
    pub op: String,
    /// Op-specific result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Fallbacks and modeling choices the engine made.
    pub warnings: Vec<GroupedWarning>,
    /// Packs, fingerprints, parameters, modes, engine.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Value>,
    /// Error, when `ok` is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiError>,
    /// Wall time.
    pub elapsed_ms: f64,
}

/// An ungrouped warning.
#[derive(Debug, Clone)]
pub(crate) struct Warn {
    pub code: String,
    pub at: Option<String>,
    pub message: String,
}

fn hint(e: &Error) -> &'static str {
    match e {
        Error::NotFound(_) => {
            r#"list ids with {"op":{"op":"graph"}} or {"op":{"op":"packs"}}; local ids work when unique"#
        }
        Error::Expr(_) => {
            r#"see {"op":{"op":"describe"}} for variables/functions; test with {"op":{"op":"metric","spec":"..."}}"#
        }
        Error::Numeric(_) => {
            "a forced or optimal choice loops forever; add a mask or policy to break the cycle"
        }
        Error::Parse(_) => {
            "see `litgraph schema <request|scenario|pack|links>` for the exact shape"
        }
        _ => "",
    }
}

/// Wall-clock timer. `std::time::Instant` panics on `wasm32-unknown-unknown`
/// (no clock), so there the timer reports 0 instead of aborting the request.
struct Stopwatch(
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))] std::time::Instant,
);

impl Stopwatch {
    fn start() -> Stopwatch {
        Stopwatch(
            #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
            std::time::Instant::now(),
        )
    }

    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    fn elapsed_ms(&self) -> f64 {
        (self.0.elapsed().as_secs_f64() * 1e6).round() / 1e3
    }

    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    #[allow(clippy::unused_self)] // no clock on this target
    fn elapsed_ms(&self) -> f64 {
        0.0
    }
}

/// Run a request. Never panics or returns `Err`: failures are `ok: false` responses.
#[must_use]
pub fn handle(req: &Request, catalog: &Catalog) -> Response {
    let t0 = Stopwatch::start();
    let op = req.op.name().to_string();
    let result = run(req, catalog);
    let elapsed_ms = t0.elapsed_ms();
    match result {
        Ok((result, warnings, provenance)) => Response {
            ok: true,
            api_version: API_VERSION,
            op,
            result: Some(result),
            warnings: render::group_warnings(warnings),
            provenance: Some(provenance),
            error: None,
            elapsed_ms,
        },
        Err(e) => Response {
            ok: false,
            api_version: API_VERSION,
            op,
            result: None,
            warnings: vec![],
            provenance: None,
            error: Some(ApiError {
                code: e.code().into(),
                message: e.to_string(),
                hint: hint(&e).into(),
            }),
            elapsed_ms,
        },
    }
}

/// Parse and run a JSON request string; malformed requests become `ok: false`.
#[must_use]
pub fn handle_json(text: &str, catalog: &Catalog) -> Response {
    match serde_json::from_str::<Request>(text) {
        Ok(req) => handle(&req, catalog),
        Err(e) => {
            let e = Error::Parse(format!("request: {e}"));
            Response {
                ok: false,
                api_version: API_VERSION,
                op: "unknown".into(),
                result: None,
                warnings: vec![],
                provenance: None,
                error: Some(ApiError {
                    code: e.code().into(),
                    message: e.to_string(),
                    hint: hint(&e).into(),
                }),
                elapsed_ms: 0.0,
            }
        }
    }
}

fn provenance(g: &Graph, selected: &[(Pack, String)], sc: &Scenario) -> Value {
    let mut params = metrics::default_params();
    params.extend(sc.params.clone());
    json!({
        "engine": ENGINE,
        "api_version": API_VERSION,
        "packs": selected.iter().map(|(p, h)| json!({ "id": p.id, "schema_version": p.schema_version, "fingerprint": h, "sources": p.sources.len() })).collect::<Vec<_>>(),
        "nodes": g.nodes.len(),
        "edges": g.edges.len(),
        "params": params,
        "modes": {
            "mixed": sc.mixed, "opponent": sc.opponent, "prob_fill": sc.prob_fill, "objective": sc.objective,
            "cost": sc.cost.clone().unwrap_or_else(|| "dollars".into()),
            "utility": sc.utility.clone().unwrap_or_else(|| "ev".into()),
        },
    })
}

type Out = (Value, Vec<Warn>, Value);

fn run(req: &Request, catalog: &Catalog) -> Result<Out> {
    if req.op == Op::Describe {
        return Ok((
            describe(catalog),
            vec![],
            json!({ "engine": ENGINE, "api_version": API_VERSION }),
        ));
    }
    let selected = catalog.select(&req.packs)?;
    let packs: Vec<Pack> = selected.iter().map(|(p, _)| p.clone()).collect();
    let links = if req.links {
        catalog.links.clone()
    } else {
        LinkFile::default()
    };
    let g = Graph::compile(
        &packs,
        &links,
        &CompileOptions {
            no_continuations: req.no_continuations,
        },
    )?;
    let prov = provenance(&g, &selected, &req.scenario);
    match &req.op {
        Op::Lint => {
            let diags = lint::lint(&g, &packs);
            return Ok((
                json!({ "count": diags.len(), "diagnostics": diags }),
                vec![],
                prov,
            ));
        }
        Op::Packs => return Ok((explore::packs_op(&g, &packs), vec![], prov)),
        Op::Batch { ops } => {
            let results: Vec<Response> = ops
                .iter()
                .map(|op| {
                    handle(
                        &Request {
                            op: op.clone(),
                            ..req.clone()
                        },
                        catalog,
                    )
                })
                .collect();
            return Ok((json!(results), vec![], prov));
        }
        Op::Compare { variant, inner } => {
            return Ok((compare(req, catalog, variant, inner)?, vec![], prov))
        }
        _ => {}
    }
    let view = View::new(&g, &req.scenario)?;
    // Every `Op` that reaches here already survived the `match` above, so
    // it can only be one of `as_view_op`'s `Some` variants — the `Lint` /
    // `Packs` / `Batch` / `Compare` (and `Describe`, handled earlier still)
    // cases all returned before this point.
    let vop = req
        .op
        .as_view_op()
        .expect("Describe/Lint/Packs/Batch/Compare all returned above");
    let (result, mut warns) = run_view(&view, &vop, &req.scenario)?;
    warns.extend(view.warnings.iter().map(|w| Warn {
        code: w.code.into(),
        at: w.at.clone(),
        message: w.message.clone(),
    }));
    warns.extend(g.notes.iter().map(|n| Warn {
        code: "compile".into(),
        at: None,
        message: n.clone(),
    }));
    Ok((result, warns, prov))
}

fn start_of(v: &View, from: Option<&String>) -> Result<usize> {
    from.map_or(Ok(v.start), |f| v.g.node(f))
}

/// Every op `handle` hands off to a resolved [`View`] — a decision op that
/// solves the game (and may warn about convergence), or a plain
/// inspection/exploration op. `op: &ViewOp` (see [`Op::as_view_op`]) makes
/// this exhaustive over exactly the ops that can reach here: no "not a
/// decision op" / "handled before the view" catch-all is reachable, or
/// needed, because `Describe`/`Lint`/`Packs`/`Batch`/`Compare` are simply
/// not expressible as a `ViewOp` in the first place.
fn run_view(v: &View, op: &ViewOp<'_>, sc: &Scenario) -> Result<(Value, Vec<Warn>)> {
    match *op {
        ViewOp::Explain { node, from } => {
            let n = match node {
                Some(x) => v.g.node(x)?,
                None => start_of(v, from.as_ref())?,
            };
            decide::explain_op(v, n)
        }
        ViewOp::Solve {
            from,
            full_policy,
            all_values,
            max_steps,
        } => decide::solve_op(
            v,
            start_of(v, from.as_ref())?,
            full_policy,
            all_values,
            max_steps,
        ),
        ViewOp::Chain { from, metrics, top } => {
            decide::chain_op(v, start_of(v, from.as_ref())?, metrics, top)
        }
        ViewOp::Simulate {
            from,
            runs,
            seed,
            metrics,
            alpha,
            max_steps,
            sample_durations,
            samples,
        } => {
            let o = SimOptions {
                runs,
                seed,
                alpha,
                max_steps,
                sample_durations,
                keep_samples: samples,
            };
            decide::simulate_op(v, start_of(v, from.as_ref())?, metrics, &o)
        }
        ViewOp::Validate => Ok((
            json!({ "valid": true, "nodes": v.g.nodes.len(), "edges": v.g.edges.len(), "start": v.g.nodes[v.start].id }),
            vec![],
        )),
        ViewOp::Graph { node } => explore::graph_op(v, node.as_deref()).map(|x| (x, vec![])),
        ViewOp::Metric { spec, top } => explore::metric_op(v, spec, top).map(|x| (x, vec![])),
        ViewOp::Path {
            from,
            to,
            metric,
            k,
            report,
        } => explore::path_op(v, start_of(v, from.as_ref())?, to, metric, k, report)
            .map(|x| (x, vec![])),
        ViewOp::Pareto {
            from,
            to,
            objectives,
            max_labels,
            limit,
        } => explore::pareto_op(v, start_of(v, from.as_ref())?, to, objectives, max_labels, limit)
            .map(|x| (x, vec![])),
        ViewOp::Sweep {
            param,
            lo,
            hi,
            steps,
            watch,
            tol,
        } => {
            let args = explore::SweepArgs {
                param,
                lo,
                hi,
                steps,
                watch,
                tol,
            };
            explore::sweep_op(v, sc, &args).map(|x| (x, vec![]))
        }
        ViewOp::Tornado {
            params,
            rel,
            dp,
            probabilities,
            top,
        } => explore::tornado_op(v.g, sc, params, rel, dp, probabilities, top)
            .map(|x| (x, vec![])),
        ViewOp::Structure {
            from,
            what,
            to,
            capacity,
            top,
        } => explore::structure_op(v, start_of(v, from.as_ref())?, what, to.as_deref(), capacity, top)
            .map(|x| (x, vec![])),
    }
}

fn compare(req: &Request, catalog: &Catalog, variant: &Value, inner: &Op) -> Result<Value> {
    let mut merged =
        serde_json::to_value(&req.scenario).map_err(|e| Error::Invalid(e.to_string()))?;
    merge_patch(&mut merged, variant);
    let variant_sc: Scenario =
        serde_json::from_value(merged).map_err(|e| Error::Parse(format!("variant: {e}")))?;
    let a = handle(
        &Request {
            op: inner.clone(),
            ..req.clone()
        },
        catalog,
    );
    let b = handle(
        &Request {
            op: inner.clone(),
            scenario: variant_sc,
            ..req.clone()
        },
        catalog,
    );
    let get = |r: &Response, k: &str| {
        r.result
            .as_ref()
            .and_then(|x| x.get(k))
            .and_then(Value::as_f64)
    };
    let delta = |k: &str| match (get(&a, k), get(&b, k)) {
        (Some(x), Some(y)) => json!(y - x),
        _ => Value::Null,
    };
    Ok(json!({
        "delta": { "value": delta("value"), "expected_utility": delta("expected_utility"), "expected_net": delta("expected_net") },
        "base": a,
        "variant": b,
    }))
}
