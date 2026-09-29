// SPDX-License-Identifier: GPL-3.0-or-later
//! Last coverage gaps in `algo::{mdp, paths, structure, sweep}`: absorption
//! through a cycle, path search and reachability past removed edges, a
//! sweep bisection midpoint that masks the decision node away, and tornado
//! analysis with a zero/absent baseline, no probability rows, and a
//! removed draw edge.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use litgraph::algo::{mdp, paths, structure, sweep};
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

fn removing(edge: &str) -> Scenario {
    Scenario {
        remove_edges: vec![edge.to_string()],
        ..Scenario::default()
    }
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

// --- algo::mdp: absorption through a cycle needs several sweeps ---

#[test]
fn absorb_prob_iterates_to_convergence_on_a_cycle() {
    // a -> t (0.5) or a -> b (0.5); b -> a. Every walk eventually absorbs.
    let g = compile(pack(json!({
        "schemaVersion": 2, "id": "cy", "title": "cy", "startNodeId": "a",
        "nodes": [
            { "id": "a", "kind": "state", "label": "a" },
            { "id": "b", "kind": "state", "label": "b" },
            { "id": "t", "kind": "terminal", "label": "t", "payoff": 0 }
        ],
        "edges": [
            { "id": "at", "from": "a", "to": "t", "label": "at", "actor": "either", "probability": 0.5 },
            { "id": "ab", "from": "a", "to": "b", "label": "ab", "actor": "either", "probability": 0.5 },
            { "id": "ba", "from": "b", "to": "a", "label": "ba", "actor": "either", "probability": 1.0 }
        ]
    })));
    let v = view(&g, &Scenario::default());
    let (a, b, t) = (
        g.node("cy::a").unwrap(),
        g.node("cy::b").unwrap(),
        g.node("cy::t").unwrap(),
    );
    let mut target = vec![0.0; g.nodes.len()];
    target[t] = 1.0;
    let p = mdp::absorb_prob(&v, &BTreeMap::new(), &target);
    assert!((p[a] - 1.0).abs() < 1e-9, "{p:?}");
    assert!((p[b] - 1.0).abs() < 1e-9, "{p:?}");
    assert!((p[t] - 1.0).abs() < 1e-9, "{p:?}");
}

// --- algo::paths / algo::structure: removed edges are skipped ---

#[test]
fn bellman_ford_skips_removed_edges() {
    let g = triangle_graph();
    let v = view(&g, &removing("ac"));
    let (a, c) = (g.node("pf::a").unwrap(), g.node("pf::c").unwrap());
    let (ab, ac, bc) = (
        g.edge("ab").unwrap(),
        g.edge("ac").unwrap(),
        g.edge("bc").unwrap(),
    );
    // The removed a->c edge would be the cheapest (-10); the negative
    // weight elsewhere forces Bellman-Ford, which must ignore it.
    let mut w = vec![0.0; g.edges.len()];
    w[ab] = -1.0;
    w[bc] = 2.0;
    w[ac] = -10.0;
    let p = paths::shortest(&v, a, c, &w).unwrap().expect("path exists");
    assert_eq!(p.edges, vec![ab, bc]);
    assert!((p.totals[0] - 1.0).abs() < 1e-9, "{p:?}");
}

#[test]
fn bellman_ford_skips_non_finite_weights() {
    let g = triangle_graph();
    let v = view(&g, &Scenario::default());
    let (a, c) = (g.node("pf::a").unwrap(), g.node("pf::c").unwrap());
    let (ab, ac, bc) = (
        g.edge("ab").unwrap(),
        g.edge("ac").unwrap(),
        g.edge("bc").unwrap(),
    );
    let mut w = vec![0.0; g.edges.len()];
    w[ab] = -1.0;
    w[bc] = 2.0;
    w[ac] = f64::NAN;
    let p = paths::shortest(&v, a, c, &w).unwrap().expect("path exists");
    assert_eq!(p.edges, vec![ab, bc]);
}

#[test]
fn pareto_ignores_removed_edges() {
    let g = triangle_graph();
    let v = view(&g, &removing("ac"));
    let (a, c) = (g.node("pf::a").unwrap(), g.node("pf::c").unwrap());
    let w = vec![1.0; g.edges.len()];
    let f = paths::pareto(&v, a, c, &[w], 100).unwrap();
    assert!(!f.truncated);
    assert_eq!(f.paths.len(), 1, "only a->b->c remains");
    assert_eq!(f.paths[0].edges.len(), 2);
}

#[test]
fn coreachable_ignores_removed_edges() {
    let g = triangle_graph();
    let v = view(&g, &removing("bc"));
    let (a, b, c) = (
        g.node("pf::a").unwrap(),
        g.node("pf::b").unwrap(),
        g.node("pf::c").unwrap(),
    );
    let seen = structure::coreachable(&v, &[c]);
    assert!(seen[a], "a still reaches c directly");
    assert!(!seen[b], "b's only route to c was removed");
    assert!(seen[c]);
}

// --- algo::sweep ---

/// Free option A pays 50; option B pays 100 but costs `100 * x` in fees.
/// B wins for x < 0.5, A after.
fn fee_decision() -> Graph {
    compile(pack(json!({
        "schemaVersion": 2, "id": "fd", "title": "fd", "startNodeId": "s",
        "roles": { "applicant": "self" },
        "nodes": [
            { "id": "s", "kind": "decision", "label": "s" },
            { "id": "ta", "kind": "terminal", "label": "ta", "payoff": 50 },
            { "id": "tb", "kind": "terminal", "label": "tb", "payoff": 100 }
        ],
        "edges": [
            { "id": "A", "from": "s", "to": "ta", "label": "A", "actor": "applicant", "cost": 0 },
            { "id": "B", "from": "s", "to": "tb", "label": "B", "actor": "applicant", "cost": 100 }
        ]
    })))
}

#[test]
fn sweep_bisection_stops_when_the_midpoint_masks_the_node_away() {
    let g = fee_decision();
    let mut sc = Scenario {
        cost: Some("fees * x".into()),
        mask: Some("if(x == 1, 0, 1)".into()),
        ..Scenario::default()
    };
    sc.params.insert("x".into(), 0.0);
    let spec = sweep::SweepSpec {
        param: "x",
        lo: 0.0,
        hi: 2.0,
        steps: 2,
        watch: &[],
        tol: 1e-6,
    };
    let r = sweep::sweep(&g, &sc, &spec).unwrap();
    assert_eq!(r.curve.len(), 2);
    // The flip is real (B at x=0, A at x=2), but the first midpoint (x=1)
    // masks every edge, so refinement stops and reports that midpoint.
    assert_eq!(r.breakpoints.len(), 1, "{:?}", r.breakpoints);
    let bp = &r.breakpoints[0];
    assert!((bp.at - 1.0).abs() < 1e-9, "{bp:?}");
    assert_eq!(bp.before, g.edge("B").unwrap());
    assert_eq!(bp.after, g.edge("A").unwrap());
}

#[test]
fn tornado_uses_a_symmetric_band_for_an_absent_or_zero_param() {
    let g = fee_decision();
    let mut sc = Scenario {
        cost: Some("fees * x".into()),
        ..Scenario::default()
    };
    // `spare` is absent from the scenario params entirely (and unused by
    // any expression), so its baseline defaults to zero.
    sc.params.insert("x".into(), 1.0);
    let (_, rows) = sweep::tornado(&g, &sc, &["spare".to_string()], 0.5, 0.1, false).unwrap();
    let row = rows.iter().find(|r| r.input == "param:spare").expect("row");
    assert!((row.low_value + 0.5).abs() < 1e-12, "{row:?}");
    assert!((row.high_value - 0.5).abs() < 1e-12, "{row:?}");
    // No probability rows when they are switched off.
    assert!(rows.iter().all(|r| r.input.starts_with("param:")));

    // An explicit zero takes the same branch.
    sc.params.insert("x".into(), 0.0);
    let (_, rows) = sweep::tornado(&g, &sc, &["x".to_string()], 0.25, 0.1, false).unwrap();
    assert!((rows[0].high_value - 0.25).abs() < 1e-12, "{:?}", rows[0]);
}

#[test]
fn tornado_skips_removed_draw_edges() {
    let g = compile(pack(json!({
        "schemaVersion": 2, "id": "dr", "title": "dr", "startNodeId": "s",
        "nodes": [
            { "id": "s", "kind": "state", "label": "s" },
            { "id": "t1", "kind": "terminal", "label": "t1", "payoff": 10 },
            { "id": "t2", "kind": "terminal", "label": "t2", "payoff": 20 },
            { "id": "t3", "kind": "terminal", "label": "t3", "payoff": 30 }
        ],
        "edges": [
            { "id": "d1", "from": "s", "to": "t1", "label": "d1", "actor": "either", "probability": 0.2 },
            { "id": "d2", "from": "s", "to": "t2", "label": "d2", "actor": "either", "probability": 0.3 },
            { "id": "d3", "from": "s", "to": "t3", "label": "d3", "actor": "either", "probability": 0.5 }
        ]
    })));
    let (_, rows) = sweep::tornado(&g, &removing("d3"), &[], 0.5, 0.1, true).unwrap();
    assert!(!rows.is_empty());
    assert!(rows.iter().all(|r| !r.input.ends_with("d3")), "{rows:?}");
    assert!(rows.iter().any(|r| r.input.ends_with("d1")), "{rows:?}");
}
