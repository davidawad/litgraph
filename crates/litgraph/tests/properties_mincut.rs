// SPDX-License-Identifier: GPL-3.0-or-later
//! Property: min cut == max flow, against a brute-force max flow on tiny graphs.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use litgraph::algo::structure;
use litgraph::model::{CompileOptions, Graph, LinkFile, Pack, RawEdge, RawNode};
use litgraph::scenario::{Scenario, View};
use proptest::prelude::*;
use rand::Rng;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

fn view<'a>(g: &'a Graph, sc: &Scenario) -> View<'a> {
    View::new(g, sc).expect("scenario resolves against a graph it was built for")
}

// ---------------------------------------------------------------------
// min-cut == max-flow, checked against a brute-force max flow on tiny
// integer-capacity DAGs.
// ---------------------------------------------------------------------

/// Recursive worker for [`brute_force_max_flow`]: tries every integer flow
/// for edge `i..`, and updates `best` whenever a complete assignment
/// conserves flow at every node but 0 and `t`.
fn brute_force_rec(
    i: usize,
    edges: &[(usize, usize, i32)],
    n: usize,
    t: usize,
    flow: &mut Vec<i32>,
    best: &mut i32,
) {
    if i == edges.len() {
        let mut balance = vec![0i64; n];
        for (k, &(a, b, _)) in edges.iter().enumerate() {
            balance[a] -= i64::from(flow[k]);
            balance[b] += i64::from(flow[k]);
        }
        for (node, &bal) in balance.iter().enumerate() {
            if node != 0 && node != t && bal != 0 {
                return;
            }
        }
        // Test graphs are tiny (<=6 edges, capacity <=3 each), so the flow
        // out of node 0 never approaches i32's range.
        #[allow(clippy::cast_possible_truncation)]
        let out_of_source = -balance[0] as i32;
        *best = (*best).max(out_of_source);
        return;
    }
    for f in 0..=edges[i].2 {
        flow[i] = f;
        brute_force_rec(i + 1, edges, n, t, flow, best);
    }
}

/// Exhaustively maximizes flow out of node 0 over all integer per-edge flows
/// in `0..=cap`, subject to conservation at every node but 0 and `t`. Only
/// tractable because these test graphs are tiny (few edges, small caps).
fn brute_force_max_flow(n: usize, edges: &[(usize, usize, i32)], t: usize) -> i32 {
    let mut best = 0;
    let mut flow = vec![0i32; edges.len()];
    brute_force_rec(0, edges, n, t, &mut flow, &mut best);
    best
}

proptest! {
    #[test]
    fn mincut_equals_bruteforce_maxflow(
        seed in any::<u64>(),
        n in 3usize..=4,
    ) {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let t = n - 1;
        // Edges only go forward (i < j), so node 0 has no in-edges and node
        // t has no out-edges: a clean s-t flow network.
        let mut raw_edges: Vec<(usize, usize, i32)> = vec![];
        for i in 0..n - 1 {
            for j in i + 1..n {
                if rng.gen_bool(0.6) {
                    raw_edges.push((i, j, rng.gen_range(1..=3)));
                }
            }
        }
        if raw_edges.is_empty() {
            return Ok(());
        }
        let brute = brute_force_max_flow(n, &raw_edges, t);

        let nodes: Vec<RawNode> = (0..n)
            .map(|i| RawNode { id: format!("n{i}"), label: format!("n{i}"), ..Default::default() })
            .collect();
        let edges: Vec<RawEdge> = raw_edges
            .iter()
            .map(|&(a, b, cap)| RawEdge {
                from: format!("n{a}"),
                to: format!("n{b}"),
                label: "e".into(),
                hours: Some(f64::from(cap)),
                ..Default::default()
            })
            .collect();
        let pack = Pack {
            schema_version: 2,
            id: "f".into(),
            title: "flow".into(),
            start_node_id: "n0".into(),
            nodes,
            edges,
            ..Default::default()
        };
        let g = Graph::compile(&[pack], &LinkFile::default(), &CompileOptions { no_continuations: true, ..CompileOptions::default() }).unwrap();
        let v = view(&g, &Scenario::default());
        let cap = v.metric("hours").unwrap();
        let cut = structure::min_cut(&v, 0, t, &cap);
        prop_assert!((cut.value - f64::from(brute)).abs() < 1e-6, "mincut {} vs bruteforce maxflow {}", cut.value, brute);
    }
}
