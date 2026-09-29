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

mod calibration;
mod catalog;
mod decide;
mod describe;
mod explore;
mod op;
mod op_defaults;
mod render;
mod stopwatch;
mod validate;

pub use calibration::{
    apply as apply_calibration, apply_all as apply_calibration_all,
    apply_named as apply_calibration_named, gaps as calibration_gaps,
    Application as CalibrationApplication, Applied as CalibrationApplied, CalibrationCatalog,
    CalibrationEntry, CalibrationSet, CalibrationSource, CalibrationTarget, Gap as CalibrationGap,
    Gaps as CalibrationGaps,
};
#[cfg(kani)]
pub(crate) use catalog::fnv1a64 as catalog_fnv1a64;
pub use catalog::{fingerprint, Catalog};
pub use describe::{describe, schema, SCHEMA_KINDS};
pub use op::{Op, StructureWhat};
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
use stopwatch::Stopwatch;

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

fn provenance(
    g: &Graph,
    selected: &[(Pack, String)],
    sc: &Scenario,
    calibration: &[CalibrationApplication],
) -> Value {
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
        // Which values were calibrated, and from where — empty unless
        // `scenario.calibration` named at least one set.
        "calibration": calibration,
    })
}

type Out = (Value, Vec<Warn>, Value);
type Answer = Result<(Value, Vec<Warn>)>;

fn run(req: &Request, catalog: &Catalog) -> Result<Out> {
    let selected = catalog.select(&req.packs)?;
    let packs: Vec<Pack> = selected.iter().map(|(p, _)| p.clone()).collect();
    let links = if req.links {
        catalog.links.clone()
    } else {
        LinkFile::default()
    };
    let mut g = Graph::compile(
        &packs,
        &links,
        &CompileOptions {
            no_continuations: req.no_continuations,
        },
    )?;
    let calibration = calibration::apply_all(&mut g, &req.scenario.calibration)?;
    let prov = provenance(&g, &selected, &req.scenario, &calibration);
    let (result, warns) = dispatch(req, catalog, &g, &packs)?;
    Ok((result, warns, prov))
}

fn plain(r: Result<Value>) -> Answer {
    r.map(|x| (x, vec![]))
}

/// Resolve the scenario into a view, run `f` on it, and attach every
/// fallback the view took plus the compile notes.
fn with_view(g: &Graph, sc: &Scenario, f: impl FnOnce(&View) -> Answer) -> Answer {
    let v = View::new(g, sc)?;
    let (result, mut warns) = f(&v)?;
    warns.extend(v.warnings.iter().map(|w| Warn {
        code: w.code.into(),
        at: w.at.clone(),
        message: w.message.clone(),
    }));
    warns.extend(g.notes.iter().map(|n| Warn {
        code: "compile".into(),
        at: None,
        message: n.clone(),
    }));
    Ok((result, warns))
}

fn start_of(v: &View, from: Option<&String>) -> Result<usize> {
    from.map_or(Ok(v.start), |f| v.g.node(f))
}

// One flat, exhaustive dispatch table over every op: splitting it would
// reintroduce "can't happen here" arms, which is what this shape avoids.
#[allow(clippy::too_many_lines)]
fn dispatch(req: &Request, catalog: &Catalog, g: &Graph, packs: &[Pack]) -> Answer {
    let sc = &req.scenario;
    match &req.op {
        Op::Describe => plain(Ok(describe(catalog))),
        Op::Lint => {
            let diags = lint::lint(g, packs);
            plain(Ok(json!({ "count": diags.len(), "diagnostics": diags })))
        }
        Op::Packs => plain(Ok(explore::packs_op(g, packs))),
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
            plain(Ok(json!(results)))
        }
        Op::Compare { variant, inner } => plain(compare(req, catalog, variant, inner)),
        Op::Validate => with_view(g, sc, |v| {
            plain(Ok(
                json!({ "valid": true, "nodes": v.g.nodes.len(), "edges": v.g.edges.len(), "start": v.g.nodes[v.start].id }),
            ))
        }),
        Op::Graph { node } => with_view(g, sc, |v| plain(explore::graph_op(v, node.as_deref()))),
        Op::Metric { spec, top } => with_view(g, sc, |v| plain(explore::metric_op(v, spec, *top))),
        Op::Explain { node, from } => with_view(g, sc, |v| {
            let n = match node {
                Some(x) => v.g.node(x)?,
                None => start_of(v, from.as_ref())?,
            };
            decide::explain_op(v, n)
        }),
        Op::Solve {
            from,
            full_policy,
            all_values,
            max_steps,
        } => with_view(g, sc, |v| {
            decide::solve_op(
                v,
                start_of(v, from.as_ref())?,
                *full_policy,
                *all_values,
                *max_steps,
            )
        }),
        Op::Chain { from, metrics, top } => with_view(g, sc, |v| {
            decide::chain_op(v, start_of(v, from.as_ref())?, metrics, *top)
        }),
        Op::Simulate {
            from,
            runs,
            seed,
            metrics,
            alpha,
            max_steps,
            sample_durations,
            samples,
        } => with_view(g, sc, |v| {
            let o = SimOptions {
                runs: *runs,
                seed: *seed,
                alpha: *alpha,
                max_steps: *max_steps,
                sample_durations: *sample_durations,
                keep_samples: *samples,
            };
            decide::simulate_op(v, start_of(v, from.as_ref())?, metrics, &o)
        }),
        Op::Path {
            from,
            to,
            metric,
            k,
            report,
        } => with_view(g, sc, |v| {
            plain(explore::path_op(
                v,
                start_of(v, from.as_ref())?,
                to,
                metric,
                *k,
                report,
            ))
        }),
        Op::Pareto {
            from,
            to,
            objectives,
            max_labels,
            limit,
        } => with_view(g, sc, |v| {
            plain(explore::pareto_op(
                v,
                start_of(v, from.as_ref())?,
                to,
                objectives,
                *max_labels,
                *limit,
            ))
        }),
        Op::Sweep {
            param,
            lo,
            hi,
            steps,
            watch,
            tol,
        } => with_view(g, sc, |v| {
            let args = explore::SweepArgs {
                param,
                lo: *lo,
                hi: *hi,
                steps: *steps,
                watch,
                tol: *tol,
            };
            plain(explore::sweep_op(v, sc, &args))
        }),
        Op::Tornado {
            params,
            rel,
            dp,
            probabilities,
            top,
        } => with_view(g, sc, |v| {
            plain(explore::tornado_op(
                v.g,
                sc,
                params,
                *rel,
                *dp,
                *probabilities,
                *top,
            ))
        }),
        Op::Calibration {
            top,
            dp,
            rel,
            max_candidates,
        } => with_view(g, sc, |v| {
            let out = calibration::gaps(v.g, sc, *dp, *rel, *max_candidates)?;
            let probabilities: Vec<&calibration::Gap> =
                out.probabilities.iter().take(*top).collect();
            let durations: Vec<&calibration::Gap> = out.durations.iter().take(*top).collect();
            plain(Ok(json!({
                "base_value": out.base_value,
                "probabilities": probabilities,
                "probabilities_truncated": out.probabilities_truncated,
                "base_elapsed_days": out.base_elapsed_days,
                "durations": durations,
                "durations_truncated": out.durations_truncated,
            })))
        }),
        Op::Structure {
            from,
            what,
            to,
            capacity,
            top,
        } => with_view(g, sc, |v| {
            plain(explore::structure_op(
                v,
                start_of(v, from.as_ref())?,
                *what,
                to.as_deref(),
                capacity,
                *top,
            ))
        }),
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
