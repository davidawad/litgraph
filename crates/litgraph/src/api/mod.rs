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
mod compare;
mod deadlines;
mod decide;
mod describe;
mod envelope;
mod explore;
mod op;
mod op_defaults;
mod render;
pub(crate) mod scenario_ref;
mod stopwatch;
mod uncertainty;
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
pub(crate) use envelope::Warn;
pub use envelope::{ApiError, GroupedWarning, Response};
pub use op::{Op, StructureWhat, StudySpec};
pub use render::choice_label;
pub use validate::{detect, validate, Validation};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::algo::sim::SimOptions;
use crate::error::{Error, Result};
use crate::lint;
use crate::metrics;
use crate::model::{CompileOptions, Graph, LinkFile, Pack, DEFAULT_MAX_PRODUCT_NODES};
use crate::scenario::{Scenario, View};
use envelope::hint;
use stopwatch::Stopwatch;

/// Engine name and version.
pub const ENGINE: &str = concat!("litgraph ", env!("CARGO_PKG_VERSION"));
/// Version of the request/response contract. Bumped on breaking changes.
pub const API_VERSION: u32 = 1;

fn yes() -> bool {
    true
}

fn default_max_product_nodes() -> usize {
    DEFAULT_MAX_PRODUCT_NODES
}

/// A request: which packs, which scenario, which operation.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Pack ids, forum keys, file stems, or paths. Empty = every pack, or —
    /// when `scenario` names a scenario that declares `packs` — that
    /// scenario's packs.
    #[serde(default)]
    pub packs: Vec<String>,
    /// Apply `links.json` between loaded packs.
    #[serde(default = "yes")]
    pub links: bool,
    /// Keep v1 absorbing-terminal semantics (no `accept` continuations).
    #[serde(default)]
    pub no_continuations: bool,
    /// Ignore every edge's `sets`/`clears`/`requires`/`forbids` state flags
    /// (v1/no-memory semantics) even on packs that declare them.
    #[serde(default)]
    pub no_flags: bool,
    /// Hard cap on compiled `(node, flag-set)` product states; a clear error
    /// above this rather than an unbounded compile. Ignored unless a pack
    /// edge declares `sets`/`clears`/`requires`/`forbids`.
    #[serde(default = "default_max_product_nodes")]
    pub max_product_nodes: usize,
    /// The matter: an inline scenario object (unknown fields rejected, as
    /// always), a string naming a scenario from the library
    /// (`litgraph describe` lists them), or `{"extends": "<name>", ...}`
    /// composing a named scenario with inline overrides deep-merged
    /// (RFC 7386) on top. See `litgraph schema request` and
    /// `docs/PACK_SCHEMA.md`.
    #[serde(default)]
    pub scenario: Value,
    /// The operation (default `describe`).
    #[serde(default)]
    pub op: Op,
}

// A hand-written `Default` (instead of `#[derive(Default)]`) so it matches
// the serde defaults above exactly: `usize::default()` is 0, which would
// make `max_product_nodes` fail closed on the very first flag-using pack.
impl Default for Request {
    fn default() -> Self {
        Request {
            packs: vec![],
            links: true,
            no_continuations: false,
            no_flags: false,
            max_product_nodes: default_max_product_nodes(),
            scenario: Value::default(),
            op: Op::default(),
        }
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
            "uncertainty": sc.uncertainty, "observe": sc.observe,
        },
        // Which values were calibrated, and from where — empty unless
        // `scenario.calibration` named at least one set.
        "calibration": calibration,
    })
}

type Out = (Value, Vec<Warn>, Value);
type Answer = Result<(Value, Vec<Warn>)>;

fn run(req: &Request, catalog: &Catalog) -> Result<Out> {
    let (sc, pack_refs) = scenario_ref::resolve(&req.scenario, &req.packs, catalog)?;
    let selected = catalog.select(&pack_refs)?;
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
            max_product_nodes: req.max_product_nodes,
            no_flags: req.no_flags,
        },
    )?;
    let calibration = calibration::apply_all(&mut g, &sc.calibration)?;
    let prov = provenance(&g, &selected, &sc, &calibration);
    let (result, warns) = dispatch(req, catalog, &g, &packs, &sc)?;
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
fn dispatch(req: &Request, catalog: &Catalog, g: &Graph, packs: &[Pack], sc: &Scenario) -> Answer {
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
        Op::Compare { variant, inner } => {
            plain(compare::compare(sc, packs, req, catalog, variant, inner))
        }
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
        Op::Posterior { .. } | Op::Voi { .. } => {
            with_view(g, sc, |v| uncertainty::dispatch(v, &req.op))
        }
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
        Op::Deadlines {
            trigger,
            edge,
            node,
            reachable,
            service_method,
            additional_holidays,
            clerk_inaccessible,
        } => deadlines::deadlines_op(
            g,
            &deadlines::DeadlinesArgs {
                trigger,
                node: node.as_deref(),
                edge: edge.as_deref(),
                reachable: *reachable,
                service_method: *service_method,
                additional_holidays,
                clerk_inaccessible: *clerk_inaccessible,
            },
        ),
    }
}
