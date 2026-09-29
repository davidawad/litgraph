// SPDX-License-Identifier: GPL-3.0-or-later
//! `describe`: the complete, machine-readable manual, and JSON Schemas.

use schemars::schema_for;
use serde_json::{json, Value};

use super::calibration::{CalibrationCatalog, CalibrationSet};
use super::catalog::Catalog;
use super::{Request, Response, API_VERSION, ENGINE};
use crate::error::{Error, Result};
use crate::metrics;
use crate::model::{LinkFile, Pack};
use crate::scenario::{NamedScenario, Scenario};

/// Documents that have a JSON Schema.
pub const SCHEMA_KINDS: &[&str] = &[
    "request",
    "response",
    "scenario",
    "named-scenario",
    "pack",
    "links",
    "calibration",
];

/// The JSON Schema of a document kind.
///
/// # Errors
/// `NotFound` for an unknown kind.
pub fn schema(kind: &str) -> Result<Value> {
    let s = match kind {
        "request" => schema_for!(Request),
        "response" => schema_for!(Response),
        "scenario" => schema_for!(Scenario),
        "named-scenario" => schema_for!(NamedScenario),
        "pack" => schema_for!(Pack),
        "links" => schema_for!(LinkFile),
        "calibration" => schema_for!(CalibrationSet),
        other => {
            return Err(Error::NotFound(format!(
                "schema {other}; kinds: {}",
                SCHEMA_KINDS.join(", ")
            )))
        }
    };
    serde_json::to_value(s).map_err(|e| Error::Invalid(e.to_string()))
}

fn builtins(xs: &[metrics::Builtin]) -> Vec<Value> {
    xs.iter()
        .map(|m| json!({ "name": m.name, "expr": m.expr, "doc": m.doc }))
        .collect()
}

fn named(xs: &[(&str, &str)]) -> Vec<Value> {
    xs.iter()
        .map(|(k, d)| json!({ "name": k, "doc": d }))
        .collect()
}

fn calibration_sets() -> (String, Vec<Value>) {
    match CalibrationCatalog::default_source() {
        Ok(c) => (
            c.origin.clone(),
            c.sets
                .iter()
                .map(
                    |(_, s, _)| json!({ "id": s.id, "title": s.title, "entries": s.entries.len() }),
                )
                .collect(),
        ),
        Err(e) => (format!("error: {e}"), vec![]),
    }
}

/// The manual: ops, scenario fields, metrics with their source expressions,
/// variables, functions, parameters, packs and instances.
#[must_use]
pub fn describe(catalog: &Catalog) -> Value {
    let (calibration_source, calibration_sets) = calibration_sets();
    json!({
        "engine": ENGINE,
        "api_version": API_VERSION,
        "packs_source": catalog.origin,
        "packs": catalog.packs.iter().map(|(_, p, _)| json!({ "id": p.id, "forum": p.forum, "title": p.title })).collect::<Vec<_>>(),
        "links": catalog.links.links.len(),
        "instances": catalog.links.instances.iter().map(|(k, i)| json!({ "id": k, "pack": i.pack, "note": i.note })).collect::<Vec<_>>(),
        "scenarios_source": catalog.scenarios_origin,
        "scenario_library": catalog.scenarios().map(|s| json!({ "id": s.id, "summary": s.summary, "packs": s.packs })).collect::<Vec<_>>(),
        "calibration_source": calibration_source,
        "calibration_sets": calibration_sets,
        "request": "see `litgraph schema request` for the full JSON Schema: {packs, links, no_continuations, scenario, op}; `scenario` is an inline object, a name from `scenario_library`, or {\"extends\": \"<name>\", ...overrides} deep-merged onto it (`litgraph schema named-scenario` for the library file shape)",
        "scenario": {
            "params": "{name: number} — visible to every expression",
            "metrics": "{name: edge-expr} — custom cost/weight functions",
            "utilities": "{name: terminal-expr}",
            "cost": "metric name or inline expr (default dollars)",
            "utility": "utility name or inline expr (default ev)",
            "perspective": "{actor: self|opponent|nature}",
            "mask": "edge-expr; edges where 0 are removed",
            "remove_edges": "[edge refs]",
            "probabilities": "{edge ref: p} (siblings rescaled)",
            "probability_fn": "edge-expr rewriting p on world edges (p = authored or NaN), chance nodes renormalized",
            "payoffs": "{node ref: usd}",
            "policy": "{node ref: edge ref} force our move",
            "facts": "{node ref: edge ref} force a `fact`-tagged chance node to its true branch (unset uses the authored prior + warns `fact-unset`)",
            "mixed": "nature-first (default; falls back to act-or-wait) | act-or-wait | self-only | optimistic",
            "opponent": "auto | adversarial | chance",
            "prob_fill": "residual | uniform",
            "objective": "{type: expected} | {type: cara, a} | {type: worst}",
            "discount_annual": "number",
            "fee_shift": "{fraction, eligible?: terminal-expr}",
            "calibration": "[calibration set id, ...] — see calibration_sets below and `litgraph schema calibration`; applied values are authored exactly as if hand-typed into the pack, reported in provenance.calibration",
            "start": "node ref",
        },
        "ops": {
            "describe": "this document",
            "packs": "packs with data-quality stats",
            "lint": "content diagnostics",
            "validate": "resolve packs + scenario without running anything",
            "graph": "{node?} nodes/edges (neighborhood if node)",
            "metric": "{spec, top?} evaluate a metric on every edge — test custom functions here",
            "explain": "{node?, from?} who decides, every option with q/regret/cost/what-happens-next",
            "solve": "{from?, full_policy?, all_values?, max_steps?} value + best line",
            "chain": "{from?, metrics?, top?} absorption probabilities + expected totals under the optimal policy",
            "simulate": "{from?, runs?, seed?, metrics?, alpha?, max_steps?, sample_durations?, samples?} outcome distribution, CVaR, P(loss)",
            "path": "{from?, to?, metric?, k?, report?} shortest / k-shortest lines; to = node | terminals | tag:<outcome>",
            "pareto": "{from?, to?, objectives?, max_labels?, limit?} N-objective frontier (default dollars × elapsed × surprise)",
            "sweep": "{param, lo?, hi?, steps?, watch?, tol?} value curve + policy breakpoints for ANY param",
            "tornado": "{params?, rel?, dp?, probabilities?, top?} what the answer is most sensitive to",
            "calibration": "{top?, dp?, rel?, max_candidates?} uncalibrated probabilities/durations ranked by decision sensitivity — what to calibrate next",
            "structure": "{what: summary|scc|dominators|mincut|betweenness|reachability, from?, to?, capacity?, top?}",
            "deadlines": "{trigger, edge?, node?, reachable?, service_method?, additional_holidays?, clerk_inaccessible?} concrete due dates + computation steps for authored deadline specs (FRCP 6 / RCFC 6 / FRAP 26 / 19 CFR 210.6(a), chosen by pack forum)",
            "compare": "{variant: scenario merge-patch, inner: op} base vs variant + deltas",
            "batch": "{ops: [op, ...]} several ops on one request",
        },
        "schemas": SCHEMA_KINDS,
        "metrics": builtins(metrics::METRICS),
        "utilities": builtins(metrics::UTILITIES),
        "params": metrics::PARAMS.iter().map(|(k, v, d)| json!({ "name": k, "default": v, "doc": d })).collect::<Vec<_>>(),
        "edge_variables": named(metrics::EDGE_VARS),
        "edge_functions": named(metrics::EDGE_FUNCS),
        "terminal_variables": named(metrics::TERMINAL_VARS),
        "math_functions": named(crate::expr::MATH_FUNCS),
        "expression_syntax": "c ? a : b, || && == != < <= > >= + - * / % ^, unary - !, calls f(x), \"strings\" as function args, identifiers may contain dots (edge.x, to.x)",
    })
}
