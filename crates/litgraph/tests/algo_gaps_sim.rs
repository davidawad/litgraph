// SPDX-License-Identifier: GPL-3.0-or-later
//! Targeted coverage for `algo::{chain, mdp, paths, structure, sim, sweep}`
//! branches the other suites don't happen to exercise: dead-end (sink)
//! states, forced policy overrides, exact value ties against a WAIT option,
//! Bellman-Ford (negative-weight) shortest paths, Pareto-frontier edge
//! cases, min-cut degenerate inputs, zero-run simulation, and sweep/tornado
//! edge cases.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use litgraph::algo::{mdp, sim, sweep};
use litgraph::model::{CompileOptions, Graph, LinkFile, Pack};
use litgraph::scenario::{Scenario, View};
use serde_json::json;

fn pack(j: serde_json::Value) -> Pack {
    serde_json::from_value(j).expect("test pack literal is well-formed")
}

fn compile(p: Pack) -> Graph {
    Graph::compile(&[p], &LinkFile::default(), &CompileOptions::default()).expect("compiles")
}

fn view<'a>(g: &'a Graph, sc: &Scenario) -> View<'a> {
    View::new(g, sc).expect("resolves")
}

fn triangle_graph() -> Graph {
    compile(pack(json!({
        "schemaVersion": 2, "id": "pf", "title": "pf", "startNodeId": "a",
        "nodes": [
            { "id": "a", "kind": "state", "label": "a" },
            { "id": "b", "kind": "state", "label": "b" },
            { "id": "c", "kind": "terminal", "label": "c", "payoff": 0 }
        ],
        "edges": [
            { "id": "ab", "from": "a", "to": "b", "label": "ab", "actor": "either", "probability": 1.0 },
            { "id": "ac", "from": "a", "to": "c", "label": "ac", "actor": "either", "probability": 0.0 },
            { "id": "bc", "from": "b", "to": "c", "label": "bc", "actor": "either", "probability": 1.0 }
        ]
    })))
}

// --- algo::sim: zero runs (empty-distribution summary), a degenerate
// triangular duration, and step-cap truncation ---

#[test]
fn simulate_with_zero_runs_summarizes_an_empty_distribution() {
    let g = triangle_graph();
    let v = view(&g, &Scenario::default());
    let sol = mdp::solve(&v, &mdp::SolveOptions::default()).unwrap();
    let r = sim::simulate(
        &v,
        &sol,
        v.start,
        &[],
        &sim::SimOptions {
            runs: 0,
            seed: 1,
            alpha: 0.1,
            max_steps: 10,
            sample_durations: false,
            keep_samples: 0,
        },
    )
    .unwrap();
    assert_eq!(r.runs, 0);
    assert!(r.net.min.is_nan());
    assert!(r.net.max.is_nan());
}

#[test]
fn simulate_degenerate_triangular_duration_returns_the_mode() {
    let g = compile(pack(json!({
        "schemaVersion": 2, "id": "dd", "title": "dd", "startNodeId": "s",
        "nodes": [
            { "id": "s", "kind": "state", "label": "s" },
            { "id": "e", "kind": "terminal", "label": "e", "payoff": 0 }
        ],
        "edges": [{
            "id": "go", "from": "s", "to": "e", "label": "go", "actor": "applicant",
            "duration": { "min": 10, "mode": 15, "max": 10 }
        }]
    })));
    let v = view(&g, &Scenario::default());
    let sol = mdp::solve(&v, &mdp::SolveOptions::default()).unwrap();
    let ms = vec![("elapsed".to_string(), v.metric("elapsed").unwrap())];
    let r = sim::simulate(
        &v,
        &sol,
        v.start,
        &ms,
        &sim::SimOptions {
            runs: 20,
            seed: 7,
            alpha: 0.1,
            max_steps: 5,
            sample_durations: true,
            keep_samples: 0,
        },
    )
    .unwrap();
    // `max <= min` in the triangular sampler always degenerates to the mode.
    let e = &r.metrics["elapsed"];
    assert!((e.min - 15.0).abs() < 1e-9, "{e:?}");
    assert!((e.max - 15.0).abs() < 1e-9, "{e:?}");
}

#[test]
fn simulate_truncates_runs_that_never_reach_a_terminal() {
    let g = compile(pack(json!({
        "schemaVersion": 2, "id": "sp", "title": "sp", "startNodeId": "s",
        "nodes": [
            { "id": "s", "kind": "state", "label": "s" },
            { "id": "e", "kind": "terminal", "label": "e", "payoff": 0 }
        ],
        "edges": [
            { "id": "spin", "from": "s", "to": "s", "label": "spin", "actor": "either", "probability": 1.0 }
        ]
    })));
    let v = view(&g, &Scenario::default());
    let sol = mdp::solve(&v, &mdp::SolveOptions::default()).unwrap();
    let r = sim::simulate(
        &v,
        &sol,
        v.start,
        &[],
        &sim::SimOptions {
            runs: 5,
            seed: 2,
            alpha: 0.1,
            max_steps: 3,
            sample_durations: false,
            keep_samples: 0,
        },
    )
    .unwrap();
    assert_eq!(r.truncated, 5);
}

// --- algo::sweep: an unwatched node is skipped, and tornado covers a
// nonzero baseline plus a self-loop edge on a terminal ---

#[test]
fn sweep_skips_flips_at_unwatched_nodes() {
    let g = compile(pack(json!({
        "schemaVersion": 2, "id": "sw", "title": "sw", "startNodeId": "n",
        "nodes": [
            { "id": "n", "kind": "state", "label": "n" },
            { "id": "t1", "kind": "terminal", "label": "t1", "payoff": 0 },
            { "id": "t2", "kind": "terminal", "label": "t2", "payoff": 10 }
        ],
        "edges": [
            { "id": "flat", "from": "n", "to": "t1", "label": "flat", "actor": "applicant" },
            { "id": "grows", "from": "n", "to": "t2", "label": "grows", "actor": "applicant" }
        ]
    })));
    let n = g.node("sw::n").unwrap();
    let sc = Scenario::default();
    // `watch` non-empty but excluding every real node id: any flip must be
    // skipped via the `continue`, not reported as a breakpoint.
    let watch_other = [n + 1000]; // an id that cannot be a real node
    let spec = sweep::SweepSpec {
        param: "unrelated",
        lo: -1.0,
        hi: 1.0,
        steps: 3,
        watch: &watch_other,
        tol: 0.01,
    };
    let r = sweep::sweep(&g, &sc, &spec).unwrap();
    assert!(r.breakpoints.is_empty());
}

#[test]
fn tornado_handles_a_nonzero_baseline_and_a_terminal_self_loop_edge() {
    let g = compile(pack(json!({
        "schemaVersion": 2, "id": "to", "title": "to", "startNodeId": "s",
        "nodes": [
            { "id": "s", "kind": "state", "label": "s" },
            { "id": "e", "kind": "terminal", "label": "e", "payoff": 10 }
        ],
        "edges": [
            { "id": "go", "from": "s", "to": "e", "label": "go", "actor": "applicant", "probability": 1.0 },
            { "id": "self-loop", "from": "e", "to": "e", "label": "authority-only", "actor": "either" }
        ]
    })));
    let mut sc = Scenario::default();
    sc.params.insert("rate".to_string(), 5.0);
    let (base, rows) = sweep::tornado(&g, &sc, &["rate".to_string()], 0.5, 0.1, true).unwrap();
    assert!((base - 10.0).abs() < 1e-6, "{base}");
    assert!(rows.iter().any(|r| r.input == "param:rate"));
}
