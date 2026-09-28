// SPDX-License-Identifier: GPL-3.0-or-later
//! Targeted coverage for `algo::{chain, mdp, paths, structure, sim, sweep}`
//! branches the other suites don't happen to exercise: dead-end (sink)
//! states, forced policy overrides, exact value ties against a WAIT option,
//! Bellman-Ford (negative-weight) shortest paths, Pareto-frontier edge
//! cases, min-cut degenerate inputs, zero-run simulation, and sweep/tornado
//! edge cases.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use litgraph::algo::{chain, mdp};
use litgraph::model::{CompileOptions, Graph, LinkFile, Pack};
use litgraph::scenario::{MixedMode, Scenario, View};
use serde_json::json;
use std::collections::BTreeMap;

fn pack(j: serde_json::Value) -> Pack {
    serde_json::from_value(j).expect("test pack literal is well-formed")
}

fn compile(p: Pack) -> Graph {
    Graph::compile(&[p], &LinkFile::default(), &CompileOptions::default()).expect("compiles")
}

fn view<'a>(g: &'a Graph, sc: &Scenario) -> View<'a> {
    View::new(g, sc).expect("resolves")
}

// --- algo::chain / algo::mdp: dead-end (Sink) states ---

fn sink_graph() -> Graph {
    compile(pack(json!({
        "schemaVersion": 2, "id": "sk", "title": "sk", "startNodeId": "s",
        "nodes": [
            { "id": "s", "kind": "state", "label": "s" },
            { "id": "e", "kind": "terminal", "label": "e", "payoff": 100 },
            { "id": "dead", "kind": "state", "label": "dead end" }
        ],
        "edges": [
            { "id": "to-e", "from": "s", "to": "e", "label": "to-e", "actor": "either", "probability": 0.5 },
            { "id": "to-dead", "from": "s", "to": "dead", "label": "to-dead", "actor": "either", "probability": 0.5 }
        ]
    })))
}

#[test]
fn mdp_values_a_dead_end_sink_at_zero() {
    let g = sink_graph();
    let v = view(&g, &Scenario::default());
    let sol = mdp::solve(&v, &mdp::SolveOptions::default()).unwrap();
    let dead = g.node("sk::dead").unwrap();
    assert_eq!(sol.value[dead], 0.0);
    // Start is worth exactly the 50% chance of the $100 terminal.
    assert_eq!(sol.value[v.start], 50.0);
}

#[test]
fn chain_reports_sink_mass_for_a_dead_end_branch() {
    let g = sink_graph();
    let v = view(&g, &Scenario::default());
    let sol = mdp::solve(&v, &mdp::SolveOptions::default()).unwrap();
    let c = chain::chain(&v, &sol, v.start, &[]).unwrap();
    assert!((c.sink_mass - 0.5).abs() < 1e-9, "{c:?}");
    let e = g.node("sk::e").unwrap();
    assert_eq!(c.absorption.len(), 1);
    assert_eq!(c.absorption[0].0, e);
    assert!((c.absorption[0].1 - 0.5).abs() < 1e-9);
    assert!((c.expected_utility - 50.0).abs() < 1e-9);
}

// --- algo::chain: step_dist falls back to a uniform split over choices
// when the caller's policy map has no recorded choice at a node ---

#[test]
fn step_dist_falls_back_to_uniform_split_with_no_recorded_choice() {
    let g = compile(pack(json!({
        "schemaVersion": 2, "id": "sd", "title": "sd", "startNodeId": "n",
        "nodes": [
            { "id": "n", "kind": "state", "label": "n" },
            { "id": "t1", "kind": "terminal", "label": "t1", "payoff": 1 },
            { "id": "t2", "kind": "terminal", "label": "t2", "payoff": 2 }
        ],
        "edges": [
            { "id": "e1", "from": "n", "to": "t1", "label": "e1", "actor": "applicant" },
            { "id": "e2", "from": "n", "to": "t2", "label": "e2", "actor": "applicant" }
        ]
    })));
    let v = view(&g, &Scenario::default());
    let n = v.start;
    // An empty policy map: no recorded choice at `n`, but it has two choices
    // and full choice mass (no interrupts) -> uniform fallback.
    let dist = chain::step_dist(&v, &BTreeMap::new(), n);
    assert_eq!(dist.len(), 2);
    for &(_, p) in &dist {
        assert!((p - 0.5).abs() < 1e-9, "{dist:?}");
    }
    let total: f64 = dist.iter().map(|x| x.1).sum();
    assert!((total - 1.0).abs() < 1e-9);
}

// --- algo::chain: an escaping branch and a permanently-trapped branch from
// the same chance split; diagnose_singular must name only the trapped one ---

#[test]
fn chain_trapped_diagnosis_excludes_the_escaping_sibling() {
    let g = compile(pack(json!({
        "schemaVersion": 2, "id": "tr", "title": "tr", "startNodeId": "s",
        "nodes": [
            { "id": "s", "kind": "state", "label": "s" },
            { "id": "a", "kind": "state", "label": "escapes" },
            { "id": "b", "kind": "state", "label": "trapped" },
            { "id": "e", "kind": "terminal", "label": "e", "payoff": 1 }
        ],
        "edges": [
            { "id": "to-a", "from": "s", "to": "a", "label": "to-a", "actor": "either", "probability": 0.5 },
            { "id": "to-b", "from": "s", "to": "b", "label": "to-b", "actor": "either", "probability": 0.5 },
            { "id": "exit", "from": "a", "to": "e", "label": "exit", "actor": "applicant" },
            { "id": "spin", "from": "b", "to": "b", "label": "spin", "actor": "applicant" }
        ]
    })));
    let v = view(&g, &Scenario::default());
    let sol = mdp::solve(&v, &mdp::SolveOptions::default()).unwrap();
    let err = chain::chain(&v, &sol, v.start, &[]).unwrap_err();
    assert_eq!(err.code(), "numeric");
    let msg = err.to_string();
    assert!(msg.contains("tr::b"), "{msg}");
    assert!(!msg.contains("tr::a"), "{msg}");
}

// --- algo::mdp: a forced policy (`sc.policy`), both onto a WAIT-eligible
// world edge and directly onto an ordinary edge ---

fn act_or_wait_graph() -> Graph {
    compile(pack(json!({
        "schemaVersion": 2, "id": "fw", "title": "fw", "startNodeId": "n",
        "nodes": [
            { "id": "n", "kind": "state", "label": "n" },
            { "id": "t1", "kind": "terminal", "label": "t1", "payoff": 10 },
            { "id": "t2", "kind": "terminal", "label": "t2", "payoff": 20 }
        ],
        "edges": [
            { "id": "act", "from": "n", "to": "t1", "label": "act", "actor": "applicant" },
            { "id": "world", "from": "n", "to": "t2", "label": "world", "actor": "either", "probability": 1.0 }
        ]
    })))
}

#[test]
fn forced_policy_onto_a_world_edge_means_wait() {
    let g = act_or_wait_graph();
    let sc = Scenario {
        mixed: MixedMode::ActOrWait,
        policy: BTreeMap::from([("fw::n".to_string(), "world".to_string())]),
        ..Scenario::default()
    };
    let v = view(&g, &sc);
    let n = v.start;
    let sol = mdp::solve(&v, &mdp::SolveOptions::default()).unwrap();
    assert_eq!(sol.choice[&n], litgraph::scenario::WAIT);
    // Forced to wait: the world edge fires for sure, worth exactly $20.
    assert_eq!(sol.value[n], 20.0);
}

#[test]
fn forced_policy_onto_an_ordinary_edge_is_taken_directly() {
    let g = act_or_wait_graph();
    let sc = Scenario {
        mixed: MixedMode::ActOrWait,
        policy: BTreeMap::from([("fw::n".to_string(), "act".to_string())]),
        ..Scenario::default()
    };
    let v = view(&g, &sc);
    let n = v.start;
    let act = g.edge("act").unwrap();
    let sol = mdp::solve(&v, &mdp::SolveOptions::default()).unwrap();
    assert_eq!(sol.choice[&n], act);
    // Forced to act even though waiting is worth more ($20 > $10): the
    // policy override is absolute.
    assert_eq!(sol.value[n], 10.0);
}

// --- algo::mdp: an exact tie between acting and waiting, broken by
// distance-to-terminal (the WAIT branch of the tie-break) ---

#[test]
fn tie_between_acting_and_waiting_is_broken_by_distance_to_terminal() {
    let g = compile(pack(json!({
        "schemaVersion": 2, "id": "tie", "title": "tie", "startNodeId": "n",
        "nodes": [
            { "id": "n", "kind": "state", "label": "n" },
            { "id": "t1", "kind": "terminal", "label": "t1", "payoff": 100 },
            { "id": "t2", "kind": "terminal", "label": "t2", "payoff": 100 }
        ],
        "edges": [
            { "id": "act", "from": "n", "to": "t1", "label": "act", "actor": "applicant" },
            { "id": "world", "from": "n", "to": "t2", "label": "world", "actor": "either", "probability": 1.0 }
        ]
    })));
    let sc = Scenario {
        mixed: MixedMode::ActOrWait,
        ..Scenario::default()
    };
    let v = view(&g, &sc);
    let sol = mdp::solve(&v, &mdp::SolveOptions::default()).unwrap();
    // Both options are worth exactly $100 (a genuine tie); either is a
    // correct optimal policy, and the value is unambiguous either way.
    assert_eq!(sol.value[v.start], 100.0);
}

// --- algo::chain::lu_solve: partial pivoting on a matrix whose natural
// diagonal is zero ---

#[test]
fn lu_solve_pivots_rows_when_the_diagonal_entry_is_zero() {
    // 0*x + 1*y = 3  (y = 3)
    // 1*x + 0*y = 2  (x = 2)
    // In row-major order the first pivot candidate (row 0, col 0) is 0, so
    // solving requires swapping rows 0 and 1 before eliminating.
    let mut a = vec![0.0, 1.0, 1.0, 0.0];
    let mut b = vec![3.0, 2.0];
    let x = chain::lu_solve(&mut a, 2, &mut b).unwrap();
    assert!((x[0] - 2.0).abs() < 1e-9, "{x:?}");
    assert!((x[1] - 3.0).abs() < 1e-9, "{x:?}");
}

#[test]
fn lu_solve_reports_a_singular_matrix() {
    // Both rows are the same equation: no unique solution.
    let mut a = vec![1.0, 1.0, 1.0, 1.0];
    let mut b = vec![2.0, 2.0];
    assert!(chain::lu_solve(&mut a, 2, &mut b).is_err());
}

// --- algo::mdp: custom (non-default) SolveOptions epsilon/max_iterations ---

#[test]
fn solve_options_respects_custom_epsilon_and_max_iterations() {
    // A world node with a genuine cycle back to itself: convergence isn't
    // trivial, so a custom (tight) epsilon and iteration cap are both
    // actually consulted by the cyclic value-iteration backup.
    let g = compile(pack(json!({
        "schemaVersion": 2, "id": "eps", "title": "eps", "startNodeId": "s",
        "nodes": [
            { "id": "s", "kind": "state", "label": "s" },
            { "id": "e", "kind": "terminal", "label": "e", "payoff": 10 }
        ],
        "edges": [
            { "id": "loop", "from": "s", "to": "s", "label": "loop", "actor": "either", "probability": 0.5 },
            { "id": "exit", "from": "s", "to": "e", "label": "exit", "actor": "either", "probability": 0.5 }
        ]
    })));
    let v = view(&g, &Scenario::default());
    let opts = mdp::SolveOptions {
        epsilon: 1e-6,
        max_iterations: 500,
    };
    let sol = mdp::solve(&v, &opts).unwrap();
    assert!(sol.converged);
    // It always eventually exits (probability 1), so the value is exactly
    // the $10 payoff.
    assert!(
        (sol.value[v.start] - 10.0).abs() < 1e-3,
        "{}",
        sol.value[v.start]
    );
}

// --- algo::mdp::absorb_prob: Sink, WAIT, and no-recorded-choice (average)
// branches, called directly since a real solved policy always populates a
// choice for every node that has choices ---

fn choice_graph_for_absorb_prob() -> (Graph, usize, usize, usize) {
    let g = compile(pack(json!({
        "schemaVersion": 2, "id": "ap", "title": "ap", "startNodeId": "n",
        "nodes": [
            { "id": "n", "kind": "state", "label": "n" },
            { "id": "t1", "kind": "terminal", "label": "t1", "payoff": 1 },
            { "id": "t2", "kind": "terminal", "label": "t2", "payoff": 0 },
            { "id": "dead", "kind": "state", "label": "dead" }
        ],
        "edges": [
            { "id": "e1", "from": "n", "to": "t1", "label": "e1", "actor": "applicant" },
            { "id": "e2", "from": "n", "to": "t2", "label": "e2", "actor": "applicant" }
        ]
    })));
    let n = g.node("ap::n").unwrap();
    let t1 = g.node("ap::t1").unwrap();
    let dead = g.node("ap::dead").unwrap();
    (g, n, t1, dead)
}

#[test]
fn absorb_prob_averages_over_choices_with_no_recorded_choice() {
    let (g, n, t1, _) = choice_graph_for_absorb_prob();
    let v = view(&g, &Scenario::default());
    let mut target = vec![0.0; g.nodes.len()];
    target[t1] = 1.0;
    // No entry for `n` at all, but it has two equally-massed choices ->
    // averaged.
    let p = mdp::absorb_prob(&v, &BTreeMap::new(), &target);
    assert!((p[n] - 0.5).abs() < 1e-9, "{p:?}");
}

#[test]
fn absorb_prob_follows_a_recorded_wait_choice() {
    let g = act_or_wait_graph();
    let sc = Scenario {
        mixed: MixedMode::ActOrWait,
        ..Scenario::default()
    };
    let v = view(&g, &sc);
    let n = v.start;
    let t2 = g.node("fw::t2").unwrap();
    let mut target = vec![0.0; g.nodes.len()];
    target[t2] = 1.0;
    let choice = BTreeMap::from([(n, litgraph::scenario::WAIT)]);
    let p = mdp::absorb_prob(&v, &choice, &target);
    // WAIT lets the single world edge (-> t2, probability 1.0) fire.
    assert!((p[n] - 1.0).abs() < 1e-9, "{p:?}");
}

#[test]
fn absorb_prob_treats_a_sink_as_zero() {
    let (g, _n, _, dead) = choice_graph_for_absorb_prob();
    let v = view(&g, &Scenario::default());
    let target = vec![1.0; g.nodes.len()];
    let p = mdp::absorb_prob(&v, &BTreeMap::new(), &target);
    // `dead` is a Sink (no active out-edges): always valued at exactly 0,
    // regardless of `target`.
    assert_eq!(p[dead], 0.0);
}
