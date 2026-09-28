// SPDX-License-Identifier: GPL-3.0-or-later
//! Adversarial review findings: each test demonstrates a suspected defect
//! with a hand-computable expected value. See the review report for the
//! ranked writeup.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use litgraph::algo::mdp;
use litgraph::model::{CompileOptions, Graph, LinkFile, Pack};
use litgraph::scenario::{Objective, Scenario, View};
use serde_json::json;

/// Bug candidate: `Objective::Worst`'s doc comment ("Nature is adversarial
/// too (robust / worst case)") is unconditional, but scenario.rs only wires
/// `worst` into the `chooser == None` branch of `View::new` (see
/// `scenario.rs:307,397` — `worst` is read exactly once). At any node where a
/// chooser (self or opponent) *also* has an out-edge, the world's edges are
/// folded into `interrupts` and go through the `MixedMode` machinery, which
/// never consults `sc.objective` at all. So wherever nature is mixed in with
/// a live chooser (the ordinary "mixed node" case central to this engine),
/// switching `Expected` -> `Worst` is a complete no-op: solve() returns the
/// bit-identical value, even though nature has genuine, authored,
/// sub-certain agency at that node.
///
/// Graph: node `start` has one self edge to `good` (+1000, cost 0) and one
/// nature edge authored p=0.4 to `bad` (-1000, cost 0) — a textbook
/// NatureFirst mixed node (chooser = self, interrupt = nature, authored,
/// mass < 1).
///
/// Expected value under plain expectation:
///   V = 0.4 * (-1000) + 0.6 * (1000) = 200.0
///
/// If `Worst` did what its own docstring promises, a robust/worst-case
/// treatment of nature's interrupt should be at least capable of pulling the
/// value away from the plain-expectation number (e.g. toward -1000 if nature
/// is assumed to always interrupt, or at minimum *some* different number).
/// Instead the actual value under `Worst` is exactly 200.0 — identical to
/// `Expected`, bit for bit.
fn mixed_node_pack() -> Graph {
    let pack: Pack = serde_json::from_value(json!({
        "schemaVersion": 2, "id": "mix", "title": "mix", "startNodeId": "start",
        "roles": { "applicant": "self", "office": "nature" },
        "nodes": [
            { "id": "start", "kind": "decision", "label": "start" },
            { "id": "good", "kind": "terminal", "label": "good", "payoff": 1000 },
            { "id": "bad", "kind": "terminal", "label": "bad", "payoff": -1000 }
        ],
        "edges": [
            { "id": "act", "from": "start", "to": "good", "label": "act", "actor": "applicant" },
            { "id": "interrupt", "from": "start", "to": "bad", "label": "interrupt", "actor": "office", "probability": 0.4 }
        ]
    }))
    .unwrap();
    Graph::compile(&[pack], &LinkFile::default(), &CompileOptions::default()).unwrap()
}

#[test]
fn worst_objective_is_a_noop_at_mixed_nature_interrupt_node() {
    let g = mixed_node_pack();

    let v_expected = View::new(&g, &Scenario::default()).unwrap();
    let sol_expected = mdp::solve(&v_expected, &mdp::SolveOptions::default()).unwrap();

    let sc_worst = Scenario {
        objective: Objective::Worst,
        ..Default::default()
    };
    let v_worst = View::new(&g, &sc_worst).unwrap();
    let sol_worst = mdp::solve(&v_worst, &mdp::SolveOptions::default()).unwrap();

    // Hand computation for Objective::Expected: 0.4*(-1000) + 0.6*(1000) = 200.
    assert!(
        (sol_expected.value[v_expected.start] - 200.0).abs() < 1e-9,
        "expected-objective value should be 200.0, got {}",
        sol_expected.value[v_expected.start]
    );

    // FAILS: demonstrates the bug. A real worst-case/robust treatment of a
    // genuine, sub-certain nature interrupt should not produce the exact
    // same number as plain expectation. It does, because `sc.objective` is
    // never consulted once a chooser (self/opponent) is also present at the
    // node.
    assert!(
        (sol_worst.value[v_worst.start] - 200.0).abs() > 1.0,
        "Objective::Worst gave {} — identical to Objective::Expected (200.0); \
         `worst` is not applied at mixed (chooser + nature-interrupt) nodes, \
         contradicting Objective::Worst's docstring ('Nature is adversarial \
         too (robust / worst case)')",
        sol_worst.value[v_worst.start]
    );
}
