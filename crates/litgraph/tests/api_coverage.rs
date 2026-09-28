// SPDX-License-Identifier: GPL-3.0-or-later
//! Exercises every `Op` (and the request/response envelope around it)
//! through `api::handle`, the CLI-agnostic surface every front end wraps.
//! Complements `tests/features.rs` (which drives `algo` directly): this file
//! is about the API layer — dispatch, rendering, schemas, error hints.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use litgraph::api::{handle, handle_json, schema, Catalog, Op, Request, StructureWhat, SCHEMA_KINDS};
use litgraph::scenario::Scenario;
use serde_json::json;

fn catalog() -> Catalog {
    Catalog::embedded().expect("embedded packs always compile")
}

fn req(op: Op) -> Request {
    Request { packs: vec!["cofc".into()], op, ..Request::default() }
}

const START: &str = "claim-accrues";
const INTERNAL_NODE: &str = "patent-claim-fork";
const TERMINAL: &str = "dismissed-time-barred";

#[test]
fn describe_lists_metrics_ops_and_schemas() {
    let resp = handle(&req(Op::Describe), &catalog());
    assert!(resp.ok);
    let r = resp.result.unwrap();
    assert!(r["ops"].is_array());
    assert!(r["metrics"].is_array());
    assert_eq!(r["schemas"], json!(SCHEMA_KINDS));
}

#[test]
fn packs_reports_data_quality() {
    let resp = handle(&req(Op::Packs), &catalog());
    assert!(resp.ok);
    let r = resp.result.unwrap();
    assert!(r.is_array() || r.is_object());
}

#[test]
fn lint_reports_diagnostics() {
    let resp = handle(&req(Op::Lint), &catalog());
    assert!(resp.ok);
    let r = resp.result.unwrap();
    assert!(r["count"].is_u64());
    assert!(r["diagnostics"].is_array());
}

#[test]
fn validate_resolves_without_running_anything() {
    let resp = handle(&req(Op::Validate), &catalog());
    assert!(resp.ok);
    assert_eq!(resp.result.unwrap()["valid"], json!(true));
}

#[test]
fn graph_default_and_node_neighborhood() {
    let resp = handle(&req(Op::Graph { node: None }), &catalog());
    assert!(resp.ok);

    let resp = handle(&req(Op::Graph { node: Some(INTERNAL_NODE.into()) }), &catalog());
    assert!(resp.ok, "{resp:?}");
}

#[test]
fn metric_top_rows() {
    let resp = handle(&req(Op::Metric { spec: "dollars".into(), top: 5 }), &catalog());
    assert!(resp.ok);
    let r = resp.result.unwrap();
    assert!(r["top"].as_array().unwrap().len() <= 5);
}

#[test]
fn explain_at_default_and_named_node() {
    let resp = handle(&req(Op::Explain { node: None, from: None }), &catalog());
    assert!(resp.ok);

    let resp = handle(&req(Op::Explain { node: Some(INTERNAL_NODE.into()), from: None }), &catalog());
    assert!(resp.ok, "{resp:?}");
}

#[test]
fn solve_full_policy_and_all_values() {
    let resp = handle(
        &req(Op::Solve { from: None, full_policy: true, all_values: true, max_steps: 40 }),
        &catalog(),
    );
    assert!(resp.ok);
    let r = resp.result.unwrap();
    assert!(r["value"].is_number());
}

#[test]
fn chain_expected_totals() {
    let resp = handle(
        &req(Op::Chain { from: None, metrics: vec!["dollars".into(), "elapsed".into()], top: 5 }),
        &catalog(),
    );
    assert!(resp.ok);
}

#[test]
fn simulate_small_run() {
    let resp = handle(
        &req(Op::Simulate {
            from: None,
            runs: 200,
            seed: 1,
            metrics: vec!["dollars".into()],
            alpha: 0.1,
            max_steps: 500,
            sample_durations: false,
            samples: 2,
        }),
        &catalog(),
    );
    assert!(resp.ok, "{resp:?}");
}

#[test]
fn path_single_and_k_shortest_with_report() {
    let resp = handle(
        &req(Op::Path { from: None, to: "terminals".into(), metric: "dollars".into(), k: 1, report: vec![] }),
        &catalog(),
    );
    assert!(resp.ok);

    let resp = handle(
        &req(Op::Path {
            from: None,
            to: "terminals".into(),
            metric: "dollars".into(),
            k: 3,
            report: vec!["elapsed".into()],
        }),
        &catalog(),
    );
    assert!(resp.ok, "{resp:?}");
}

#[test]
fn pareto_frontier_multi_objective() {
    let resp = handle(
        &req(Op::Pareto {
            from: None,
            to: "terminals".into(),
            objectives: vec!["dollars".into(), "elapsed".into()],
            max_labels: 5_000,
            limit: 10,
        }),
        &catalog(),
    );
    assert!(resp.ok, "{resp:?}");
}

#[test]
fn sweep_finds_a_curve() {
    let resp = handle(
        &req(Op::Sweep { param: "rate".into(), lo: 100.0, hi: 1000.0, steps: 5, watch: vec![], tol: 1e-3 }),
        &catalog(),
    );
    assert!(resp.ok, "{resp:?}");
}

#[test]
fn tornado_sensitivity() {
    let resp = handle(
        &req(Op::Tornado { params: vec!["rate".into()], rel: 0.25, dp: 0.1, probabilities: true, top: 5 }),
        &catalog(),
    );
    assert!(resp.ok, "{resp:?}");
}

#[test]
fn structure_every_analysis() {
    let cat = catalog();
    let cases = [
        Op::Structure { from: None, what: StructureWhat::Summary, to: None, capacity: "1".into(), top: 5 },
        Op::Structure { from: None, what: StructureWhat::Scc, to: None, capacity: "1".into(), top: 5 },
        Op::Structure { from: None, what: StructureWhat::Dominators, to: None, capacity: "1".into(), top: 5 },
        Op::Structure {
            from: None,
            what: StructureWhat::Dominators,
            to: Some(TERMINAL.into()),
            capacity: "1".into(),
            top: 5,
        },
        Op::Structure {
            from: None,
            what: StructureWhat::Mincut,
            to: Some(TERMINAL.into()),
            capacity: "1".into(),
            top: 5,
        },
        Op::Structure { from: None, what: StructureWhat::Betweenness, to: None, capacity: "1".into(), top: 5 },
        Op::Structure { from: None, what: StructureWhat::Reachability, to: None, capacity: "1".into(), top: 5 },
    ];
    for op in cases {
        let resp = handle(&req(op.clone()), &cat);
        assert!(resp.ok, "{op:?} -> {resp:?}");
    }
}

#[test]
fn structure_mincut_without_to_is_an_error() {
    let resp = handle(
        &req(Op::Structure { from: None, what: StructureWhat::Mincut, to: None, capacity: "1".into(), top: 5 }),
        &catalog(),
    );
    assert!(!resp.ok);
    assert_eq!(resp.error.unwrap().code, "invalid");
}

#[test]
fn compare_base_and_variant_deltas() {
    let resp = handle(
        &req(Op::Compare {
            variant: json!({ "params": { "rate": 900.0 } }),
            inner: Box::new(Op::Chain { from: None, metrics: vec!["dollars".into()], top: 5 }),
        }),
        &catalog(),
    );
    assert!(resp.ok, "{resp:?}");
    let r = resp.result.unwrap();
    assert!(r["base"]["ok"].as_bool().unwrap());
    assert!(r["variant"]["ok"].as_bool().unwrap());
}

#[test]
fn batch_runs_every_sub_op() {
    let resp = handle(&req(Op::Batch { ops: vec![Op::Describe, Op::Packs, Op::Lint] }), &catalog());
    assert!(resp.ok);
    assert_eq!(resp.result.unwrap().as_array().unwrap().len(), 3);
}

#[test]
fn scenario_scoped_start_and_perspective_flow_through() {
    let sc = Scenario { start: Some(START.into()), ..Scenario::default() };
    let solve = Op::Solve { from: None, full_policy: false, all_values: false, max_steps: 40 };
    let request = Request { packs: vec!["cofc".into()], scenario: sc, op: solve, ..Request::default() };
    let resp = handle(&request, &catalog());
    assert!(resp.ok, "{resp:?}");
}

// --- error paths: unknown pack, bad node, bad expression, malformed JSON ---

#[test]
fn unknown_pack_ref_is_not_found_with_a_hint() {
    let request = Request { packs: vec!["does-not-exist".into()], op: Op::Packs, ..Request::default() };
    let resp = handle(&request, &catalog());
    assert!(!resp.ok);
    let err = resp.error.unwrap();
    assert_eq!(err.code, "not-found");
    assert!(!err.hint.is_empty());
}

#[test]
fn unknown_node_ref_is_not_found() {
    let resp = handle(&req(Op::Graph { node: Some("does-not-exist".into()) }), &catalog());
    assert!(!resp.ok);
    assert_eq!(resp.error.unwrap().code, "not-found");
}

#[test]
fn bad_expression_is_an_expr_error_with_a_hint() {
    let resp = handle(&req(Op::Metric { spec: "no_such_variable_at_all".into(), top: 5 }), &catalog());
    assert!(!resp.ok);
    let err = resp.error.unwrap();
    assert_eq!(err.code, "expr");
    assert!(err.hint.contains("describe"));
}

#[test]
fn malformed_json_request_is_a_parse_error() {
    let resp = handle_json("{ not json", &catalog());
    assert!(!resp.ok);
    assert_eq!(resp.error.unwrap().code, "parse");
    assert_eq!(resp.op, "unknown");
}

#[test]
fn well_formed_json_request_round_trips_through_handle_json() {
    let resp = handle_json(r#"{"packs":["cofc"],"op":{"op":"packs"}}"#, &catalog());
    assert!(resp.ok);
}

// --- schema() / SCHEMA_KINDS: the CLI's `schema <kind>` surface ---

#[test]
fn schema_returns_every_declared_kind() {
    for kind in SCHEMA_KINDS {
        let v = schema(kind).unwrap_or_else(|e| panic!("schema({kind}) failed: {e}"));
        assert!(v.is_object(), "schema({kind}) should be a JSON schema object");
    }
}

#[test]
fn schema_rejects_an_unknown_kind() {
    let err = schema("not-a-kind").unwrap_err();
    assert_eq!(err.code(), "not-found");
}
