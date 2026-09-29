// SPDX-License-Identifier: GPL-3.0-or-later
//! Monte Carlo fee shifting: recovery applies only at fee-eligible
//! terminals, eligibility can be a custom expression, and a broken
//! eligibility expression is an error rather than a silent "nobody".

#![allow(clippy::unwrap_used, clippy::expect_used)]

use litgraph::algo::{mdp, sim};
use litgraph::model::{CompileOptions, Graph, LinkFile, Pack};
use litgraph::scenario::{FeeShift, Scenario, View};
use serde_json::json;

/// Pay 100, then a fair coin lands on an eligible ("fee-eligible" outcome)
/// or an ineligible terminal, both worth 0.
fn graph() -> Graph {
    let p: Pack = serde_json::from_value(json!({
        "schemaVersion": 2, "id": "fee", "title": "fee", "startNodeId": "start",
        "roles": { "applicant": "self", "office": "nature" },
        "nodes": [
            { "id": "start", "kind": "decision", "label": "start" },
            { "id": "flip", "kind": "state", "label": "flip" },
            { "id": "ok", "kind": "terminal", "label": "ok", "payoff": 0, "outcome": ["fee-eligible"] },
            { "id": "no", "kind": "terminal", "label": "no", "payoff": 0 }
        ],
        "edges": [
            { "id": "pay", "from": "start", "to": "flip", "label": "pay", "actor": "applicant", "cost": 100 },
            { "id": "toOk", "from": "flip", "to": "ok", "label": "ok", "actor": "office", "probability": 0.5 },
            { "id": "toNo", "from": "flip", "to": "no", "label": "no", "actor": "office", "probability": 0.5 }
        ]
    }))
    .expect("pack parses");
    Graph::compile(&[p], &LinkFile::default(), &CompileOptions::default()).expect("compiles")
}

fn options() -> sim::SimOptions {
    sim::SimOptions {
        runs: 2000,
        seed: 7,
        alpha: 0.1,
        max_steps: 10,
        sample_durations: false,
        keep_samples: 0,
    }
}

fn run(eligible: Option<&str>) -> litgraph::Result<sim::SimResult> {
    let g = graph();
    let sc = Scenario {
        fee_shift: Some(FeeShift {
            fraction: 0.5,
            eligible: eligible.map(String::from),
        }),
        ..Scenario::default()
    };
    let v = View::new(&g, &sc)?;
    let sol = mdp::solve(&v, &mdp::SolveOptions::default())?;
    sim::simulate(&v, &sol, v.start, &[], &options())
}

#[test]
fn default_eligibility_recovers_half_the_spend_only_at_tagged_terminals() {
    let r = run(None).unwrap();
    // Eligible: -100 + 50; ineligible: -100.
    assert!((r.net.min + 100.0).abs() < 1e-9, "{:?}", r.net);
    assert!((r.net.max + 50.0).abs() < 1e-9, "{:?}", r.net);
    assert!((r.net.mean + 75.0).abs() < 5.0, "{:?}", r.net);
}

#[test]
fn custom_eligibility_expression_selects_terminals() {
    // Every terminal eligible: always recover half.
    let all = run(Some("1")).unwrap();
    assert!((all.net.min + 50.0).abs() < 1e-9);
    assert!((all.net.max + 50.0).abs() < 1e-9);
    // None eligible: never recover.
    let none = run(Some("0")).unwrap();
    assert!((none.net.min + 100.0).abs() < 1e-9);
    assert!((none.net.max + 100.0).abs() < 1e-9);
}

#[test]
fn broken_eligibility_expression_is_an_error() {
    assert!(run(Some("no_such_fn(1)")).is_err());
}
