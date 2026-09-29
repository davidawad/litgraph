// SPDX-License-Identifier: GPL-3.0-or-later
//! `sweep` is genuinely fallible: it re-resolves the scenario at every grid
//! point, so a swept parameter that breaks an expression only at some values
//! is an error the base scenario (which resolves fine) never shows.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use litgraph::api::{handle, Catalog, Op, Request};
use litgraph::model::Pack;
use litgraph::scenario::Scenario;
use serde_json::json;

fn catalog() -> Catalog {
    let pack: Pack = serde_json::from_value(json!({
        "schemaVersion": 2, "id": "chancey", "title": "chancey", "startNodeId": "start",
        "roles": { "applicant": "self", "office": "nature" },
        "nodes": [
            {"id": "start", "kind": "decision", "label": "Start"},
            {"id": "flip", "kind": "state", "label": "Flip"},
            {"id": "up", "kind": "terminal", "label": "Up", "payoff": 10},
            {"id": "down", "kind": "terminal", "label": "Down", "payoff": -10}
        ],
        "edges": [
            {"id": "go", "from": "start", "to": "flip", "label": "Go", "actor": "applicant"},
            {"id": "toUp", "from": "flip", "to": "up", "label": "Up", "actor": "office", "probability": 0.5},
            {"id": "toDown", "from": "flip", "to": "down", "label": "Down", "actor": "office", "probability": 0.5}
        ]
    }))
    .expect("pack parses");
    Catalog::from_files(
        "t".into(),
        vec![("t.json".into(), serde_json::to_string(&pack).unwrap())],
    )
    .expect("catalog compiles")
}

/// The base scenario (`x = 100`) resolves; the expression only calls an
/// unknown function once the swept `x` passes 500.
fn sweep_request(hi: f64) -> Request {
    Request {
        packs: vec!["chancey".into()],
        scenario: Scenario {
            params: [("x".to_string(), 100.0)].into(),
            probability_fn: Some("if(x > 500, no_such_fn(p), p)".into()),
            ..Scenario::default()
        },
        op: Op::Sweep {
            param: "x".into(),
            lo: 100.0,
            hi,
            steps: 5,
            watch: vec![],
            tol: 1e-6,
        },
        ..Request::default()
    }
}

#[test]
fn sweep_reports_the_expression_error_from_a_grid_point() {
    let resp = handle(&sweep_request(1000.0), &catalog());
    assert!(!resp.ok, "{resp:?}");
    let e = resp.error.unwrap();
    assert_eq!(e.code, "expr");
    assert!(e.message.contains("no_such_fn"), "{}", e.message);
}

#[test]
fn sweep_within_the_safe_range_succeeds() {
    let resp = handle(&sweep_request(400.0), &catalog());
    assert!(resp.ok, "{resp:?}");
    assert_eq!(resp.result.unwrap()["curve"].as_array().unwrap().len(), 5);
}
