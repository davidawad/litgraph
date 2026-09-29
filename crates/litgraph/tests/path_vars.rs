// SPDX-License-Identifier: GPL-3.0-or-later
//! Coverage for path-dependent terminal variables (`spent`, `elapsed_total`,
//! `steps`): available to terminal expressions, exact in `simulate`, and
//! `0` (with a warning) in the Markov `solve`/`chain` path. See
//! `docs/COST_FUNCTIONS.md`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use litgraph::algo::{mdp, sim};
use litgraph::model::{CompileOptions, Graph, LinkFile, Pack};
use litgraph::scenario::{Scenario, View};
use serde_json::json;

fn pack(j: serde_json::Value) -> Pack {
    serde_json::from_value(j).expect("test pack literal is well-formed")
}

fn compile(p: Pack) -> Graph {
    Graph::compile(&[p], &LinkFile::default(), &CompileOptions::default()).expect("compiles")
}

/// A deterministic two-edge chain `start -> mid -> end`, each edge with a
/// known cost and elapsed duration, ending at a terminal with `payoff`.
fn chain_graph(payoff: f64) -> Graph {
    compile(pack(json!({
        "schemaVersion": 2, "id": "pv", "title": "pv", "startNodeId": "start",
        "nodes": [
            { "id": "start", "kind": "state", "label": "start" },
            { "id": "mid", "kind": "state", "label": "mid" },
            { "id": "end", "kind": "terminal", "label": "end", "payoff": payoff }
        ],
        "edges": [
            { "id": "e1", "from": "start", "to": "mid", "label": "e1", "actor": "applicant",
              "cost": 100.0, "duration": { "mode": 30 } },
            { "id": "e2", "from": "mid", "to": "end", "label": "e2", "actor": "applicant",
              "cost": 50.0, "duration": { "mode": 60 } }
        ]
    })))
}

fn interest_scenario(rate: f64) -> Scenario {
    let mut sc = Scenario::default();
    sc.params.insert("r".to_string(), rate);
    // Prejudgment interest compounding on elapsed calendar time.
    sc.utility = Some("payoff * (1 + r) ^ (elapsed_total / 365)".to_string());
    sc
}

#[test]
fn solve_and_chain_treat_path_vars_as_zero_and_warn() {
    let g = chain_graph(1000.0);
    let sc = interest_scenario(0.10);
    let v = View::new(&g, &sc).unwrap();
    // elapsed_total defaults to 0 in the Markov utility, so `(1+r)^0 == 1`:
    // the utility is exactly the payoff, un-grown.
    let end = g.node("pv::end").unwrap();
    assert_eq!(v.utility[end], 1000.0);

    let sol = mdp::solve(&v, &mdp::SolveOptions::default()).unwrap();
    // start -(cost 100)-> mid -(cost 50)-> end(1000): value = 1000 - 150.
    assert!(
        (sol.value[v.start] - 850.0).abs() < 1e-9,
        "{}",
        sol.value[v.start]
    );

    assert!(
        v.warnings
            .iter()
            .any(|w| w.code == "path-variable-in-markov" && w.message.contains("elapsed_total")),
        "{:?}",
        v.warnings
    );
}

#[test]
fn simulate_computes_exact_prejudgment_interest_along_the_sampled_path() {
    let g = chain_graph(1000.0);
    let sc = interest_scenario(0.10);
    let v = View::new(&g, &sc).unwrap();
    let sol = mdp::solve(&v, &mdp::SolveOptions::default()).unwrap();
    let res = sim::simulate(
        &v,
        &sol,
        v.start,
        &[],
        &sim::SimOptions {
            runs: 5,
            seed: 1,
            alpha: 0.5,
            max_steps: 10,
            sample_durations: false, // fixed `mode` durations: elapsed_total is deterministic
            keep_samples: 0,
        },
    )
    .unwrap();
    // Deterministic graph (no chance nodes): every run takes e1 then e2.
    // elapsed_total = 30 + 60 = 90 days; spent = 100 + 50 = 150.
    let expected_utility = 1000.0 * 1.10_f64.powf(90.0 / 365.0);
    let expected_net = expected_utility - 150.0;
    assert!(
        (res.net.mean - expected_net).abs() < 1e-6,
        "mean={} expected={expected_net}",
        res.net.mean
    );
    // The un-grown (Markov) value from `solve` differs (no interest, so the
    // path-dependent net is strictly higher for a positive rate).
    assert!(expected_net > sol.value[v.start]);
}

#[test]
fn steps_variable_counts_edges_traversed() {
    let g = chain_graph(0.0);
    let sc = Scenario {
        utility: Some("steps".to_string()),
        ..Default::default()
    };
    let v = View::new(&g, &sc).unwrap();
    let sol = mdp::solve(&v, &mdp::SolveOptions::default()).unwrap();
    let res = sim::simulate(
        &v,
        &sol,
        v.start,
        &[],
        &sim::SimOptions {
            runs: 3,
            seed: 9,
            alpha: 0.5,
            max_steps: 10,
            sample_durations: false,
            keep_samples: 0,
        },
    )
    .unwrap();
    // Two edges to the terminal, plus the 150 in cost netted out: net = 2 - 150.
    assert!(
        (res.net.mean - (2.0 - 150.0)).abs() < 1e-9,
        "{}",
        res.net.mean
    );
}

#[test]
fn fee_shift_eligible_referencing_path_vars_also_warns() {
    let g = chain_graph(1000.0);
    let sc = Scenario {
        fee_shift: Some(litgraph::scenario::FeeShift {
            fraction: 0.5,
            eligible: Some("spent > 1".to_string()),
        }),
        ..Default::default()
    };
    let v = View::new(&g, &sc).unwrap();
    assert!(
        v.warnings
            .iter()
            .any(|w| w.code == "path-variable-in-markov" && w.message.contains("fee_shift")),
        "{:?}",
        v.warnings
    );
}

#[test]
fn a_utility_without_path_vars_gets_no_warning() {
    let g = chain_graph(1000.0);
    let v = View::new(&g, &Scenario::default()).unwrap();
    assert!(!v
        .warnings
        .iter()
        .any(|w| w.code == "path-variable-in-markov"));
}
