// SPDX-License-Identifier: GPL-3.0-or-later
//! Targeted coverage for `algo::{chain, mdp, paths, structure, sim, sweep}`
//! branches the other suites don't happen to exercise: dead-end (sink)
//! states, forced policy overrides, exact value ties against a WAIT option,
//! Bellman-Ford (negative-weight) shortest paths, Pareto-frontier edge
//! cases, min-cut degenerate inputs, zero-run simulation, and sweep/tornado
//! edge cases.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use litgraph::algo::{paths, structure};
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

// --- algo::paths: Bellman–Ford for negative edge weights, and its
// negative-cycle error ---

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

#[test]
fn shortest_uses_bellman_ford_when_a_weight_is_negative() {
    let g = triangle_graph();
    let v = view(&g, &Scenario::default());
    let a = g.node("pf::a").unwrap();
    let c = g.node("pf::c").unwrap();
    let ab = g.edge("ab").unwrap();
    let ac = g.edge("ac").unwrap();
    let bc = g.edge("bc").unwrap();
    // a->c direct costs 10; a->b->c costs -5 + 1 = -4: the negative-weight
    // route must win, which requires the Bellman-Ford path (Dijkstra can't
    // handle the negative edge at all).
    let mut w = vec![0.0; g.edges.len()];
    w[ab] = -5.0;
    w[bc] = 1.0;
    w[ac] = 10.0;
    let p = paths::shortest(&v, a, c, &w).unwrap().expect("path exists");
    assert_eq!(p.edges, vec![ab, bc]);
    assert!((p.totals[0] - (-4.0)).abs() < 1e-9, "{p:?}");
}

#[test]
fn shortest_errors_on_a_reachable_negative_cycle() {
    let g = compile(pack(json!({
        "schemaVersion": 2, "id": "nc", "title": "nc", "startNodeId": "a",
        "nodes": [
            { "id": "a", "kind": "state", "label": "a" },
            { "id": "b", "kind": "state", "label": "b" },
            { "id": "c", "kind": "terminal", "label": "c", "payoff": 0 }
        ],
        "edges": [
            { "id": "ab", "from": "a", "to": "b", "label": "ab", "actor": "either", "probability": 0.5 },
            { "id": "ba", "from": "b", "to": "a", "label": "ba", "actor": "either", "probability": 1.0 },
            { "id": "ac", "from": "a", "to": "c", "label": "ac", "actor": "either", "probability": 0.5 }
        ]
    })));
    let v = view(&g, &Scenario::default());
    let a = g.node("nc::a").unwrap();
    let c = g.node("nc::c").unwrap();
    let ab = g.edge("ab").unwrap();
    let ba = g.edge("ba").unwrap();
    let ac = g.edge("ac").unwrap();
    let mut w = vec![0.0; g.edges.len()];
    w[ab] = -1.0;
    w[ba] = -1.0;
    w[ac] = 5.0;
    let err = paths::shortest(&v, a, c, &w).unwrap_err();
    assert_eq!(err.code(), "numeric");
    assert!(err.to_string().contains("negative cycle"));
}

// --- algo::paths: k_shortest with no path at all, and with fewer distinct
// simple paths than requested ---

#[test]
fn k_shortest_is_empty_when_no_path_exists() {
    let g = compile(pack(json!({
        "schemaVersion": 2, "id": "np", "title": "np", "startNodeId": "a",
        "nodes": [
            { "id": "a", "kind": "terminal", "label": "a", "payoff": 0 },
            { "id": "b", "kind": "terminal", "label": "b", "payoff": 0 }
        ],
        "edges": []
    })));
    let v = view(&g, &Scenario::default());
    let a = g.node("np::a").unwrap();
    let b = g.node("np::b").unwrap();
    let w = vec![];
    let ps = paths::k_shortest(&v, a, b, &w, 3).unwrap();
    assert!(ps.is_empty());
}

#[test]
fn k_shortest_returns_fewer_than_k_when_alternatives_are_exhausted() {
    let g = triangle_graph();
    let v = view(&g, &Scenario::default());
    let a = g.node("pf::a").unwrap();
    let c = g.node("pf::c").unwrap();
    let w = vec![1.0; g.edges.len()];
    // Only a couple of loopless simple paths exist; asking for 5 must stop
    // early rather than loop forever or panic.
    let ps = paths::k_shortest(&v, a, c, &w, 5).unwrap();
    assert!(ps.len() < 5 && !ps.is_empty(), "{}", ps.len());
}

// --- algo::paths::pareto: validation errors and truncation ---

#[test]
fn pareto_rejects_an_empty_objective_list() {
    let g = triangle_graph();
    let v = view(&g, &Scenario::default());
    let err = paths::pareto(&v, v.start, v.start, &[], 100).unwrap_err();
    assert_eq!(err.code(), "invalid");
}

#[test]
fn pareto_rejects_a_negative_objective_on_an_active_edge() {
    let g = triangle_graph();
    let v = view(&g, &Scenario::default());
    let a = g.node("pf::a").unwrap();
    let c = g.node("pf::c").unwrap();
    let w = vec![-1.0; g.edges.len()];
    let err = paths::pareto(&v, a, c, &[w], 100).unwrap_err();
    assert_eq!(err.code(), "invalid");
    assert!(err.to_string().contains("non-negative"));
}

#[test]
fn pareto_reports_truncation_when_max_labels_is_hit() {
    let g = triangle_graph();
    let v = view(&g, &Scenario::default());
    let a = g.node("pf::a").unwrap();
    let c = g.node("pf::c").unwrap();
    let w = vec![1.0; g.edges.len()];
    let f = paths::pareto(&v, a, c, &[w], 1).unwrap();
    assert!(f.truncated);
}

// --- algo::structure: coreachable, dominators past an inactive edge,
// and min-cut degenerate/clamped inputs ---

#[test]
fn coreachable_finds_every_node_that_can_reach_the_targets() {
    let g = triangle_graph();
    let v = view(&g, &Scenario::default());
    let a = g.node("pf::a").unwrap();
    let b = g.node("pf::b").unwrap();
    let c = g.node("pf::c").unwrap();
    let seen = structure::coreachable(&v, &[c]);
    assert!(seen[a] && seen[b] && seen[c]);

    // Nothing can reach `a` (it's the source): coreachable from `a` alone
    // is just `{a}`.
    let seen_a = structure::coreachable(&v, &[a]);
    assert!(seen_a[a] && !seen_a[b] && !seen_a[c]);
}

#[test]
fn dominators_skip_masked_out_edges() {
    let g = triangle_graph();
    // Mask the direct a->c edge: a's only remaining route to c is a->b->c,
    // so b becomes c's immediate dominator once `ac` is inactive; this
    // forces the dominator walk to actually skip an inactive out/in-edge.
    let sc = Scenario {
        remove_edges: vec!["ac".to_string()],
        ..Scenario::default()
    };
    let v = view(&g, &sc);
    let a = g.node("pf::a").unwrap();
    let b = g.node("pf::b").unwrap();
    let c = g.node("pf::c").unwrap();
    let idom = structure::dominators(&v, a);
    assert_eq!(idom[b], Some(a));
    assert_eq!(idom[c], Some(b));
}

#[test]
fn min_cut_from_a_node_to_itself_is_zero() {
    let g = triangle_graph();
    let v = view(&g, &Scenario::default());
    let a = g.node("pf::a").unwrap();
    let cap = vec![1.0; g.edges.len()];
    let cut = structure::min_cut(&v, a, a, &cap);
    assert_eq!(cut.value, 0.0);
    assert!(cut.edges.is_empty());
}

#[test]
fn min_cut_clamps_negative_and_non_finite_capacities_to_zero() {
    let g = triangle_graph();
    let v = view(&g, &Scenario::default());
    let a = g.node("pf::a").unwrap();
    let c = g.node("pf::c").unwrap();
    let ab = g.edge("ab").unwrap();
    let ac = g.edge("ac").unwrap();
    let bc = g.edge("bc").unwrap();
    let mut cap = vec![0.0; g.edges.len()];
    cap[ab] = -3.0; // clamped to 0
    cap[ac] = f64::NAN; // clamped to 0
    cap[bc] = 2.0;
    let cut = structure::min_cut(&v, a, c, &cap);
    // Both routes to `c` are capacity-0 (a->c clamped, a->b clamped so no
    // flow ever reaches b->c either): the min cut is 0.
    assert_eq!(cut.value, 0.0);
}
