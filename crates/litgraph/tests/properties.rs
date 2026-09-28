// SPDX-License-Identifier: GPL-3.0-or-later
//! Property-based tests (proptest) over the expression language, probability
//! filling, and the core algorithms (mdp, chain, sim, paths, structure) on
//! randomly generated small acyclic stochastic graphs.
//!
//! Graphs are generated deterministically from a `u64` seed rather than via a
//! custom `proptest::Strategy` — simpler to write and read, and proptest's
//! shrinker still narrows failures to a smaller seed even though "smaller"
//! has no direct structural meaning here.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use litgraph::algo::{chain, mdp, paths, structure};
use litgraph::expr;
use litgraph::model::{CompileOptions, Graph, LinkFile, NodeKind, Pack, RawEdge, RawNode};
use litgraph::scenario::{fill, MixedMode, Objective, ProbFill, Scenario, View};
use proptest::prelude::*;
use rand::Rng;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

// ---------------------------------------------------------------------
// A small random acyclic chance graph: states s0..s{n-1}, terminals
// t0..t{m-1}. Every state's out-edges go to a strictly higher state or to a
// terminal (so the graph is a DAG and every state absorbs), all edges are
// `either` (Nature) with authored probabilities that sum to exactly 1 at
// each state.
// ---------------------------------------------------------------------

struct RandomChain {
    graph: Graph,
    n_states: usize,
    n_terms: usize,
}

fn random_chain(rng: &mut ChaCha8Rng, n_states: usize, n_terms: usize) -> RandomChain {
    let mut nodes = vec![];
    for i in 0..n_states {
        nodes.push(RawNode {
            id: format!("s{i}"),
            label: format!("state {i}"),
            ..Default::default()
        });
    }
    for t in 0..n_terms {
        nodes.push(RawNode {
            id: format!("t{t}"),
            label: format!("terminal {t}"),
            kind: Some(NodeKind::Terminal),
            payoff: Some(rng.gen_range(-10_000.0..=10_000.0)),
            ..Default::default()
        });
    }
    let mut edges = vec![];
    for i in 0..n_states {
        // Candidate targets: any later state, or any terminal.
        let mut candidates: Vec<String> = (i + 1..n_states).map(|j| format!("s{j}")).collect();
        candidates.extend((0..n_terms).map(|t| format!("t{t}")));
        let k = rng.gen_range(1..=3.min(candidates.len()).max(1));
        // Sample k distinct candidates (candidates is always non-empty: at
        // least one terminal exists).
        let mut chosen = vec![];
        let mut pool = candidates.clone();
        for _ in 0..k.min(pool.len()) {
            let idx = rng.gen_range(0..pool.len());
            chosen.push(pool.remove(idx));
        }
        let weights: Vec<f64> = chosen.iter().map(|_| rng.gen_range(1.0..=5.0)).collect();
        let sum: f64 = weights.iter().sum();
        for (to, w) in chosen.into_iter().zip(weights) {
            edges.push(RawEdge {
                from: format!("s{i}"),
                to,
                label: "step".into(),
                probability: Some(w / sum),
                hours: Some(rng.gen_range(0.0..=10.0)),
                cost: Some(rng.gen_range(0.0..=500.0)),
                ..Default::default()
            });
        }
    }
    let pack = Pack {
        schema_version: 2,
        id: "p".into(),
        title: "random".into(),
        start_node_id: "s0".into(),
        nodes,
        edges,
        ..Default::default()
    };
    let graph = Graph::compile(&[pack], &LinkFile::default(), &CompileOptions { no_continuations: true })
        .expect("random chain packs always compile");
    RandomChain { graph, n_states, n_terms }
}

fn view(g: &Graph, sc: &Scenario) -> View<'_> {
    View::new(g, sc).expect("scenario resolves against a graph it was built for")
}

// ---------------------------------------------------------------------
// expr: never panics, round-trips numeric literals.
// ---------------------------------------------------------------------

struct EmptyEnv;
impl expr::Env for EmptyEnv {
    fn var(&self, _: &str) -> Option<f64> {
        None
    }
    fn func(&self, _: &str, _: &[expr::Arg]) -> Option<litgraph::Result<f64>> {
        None
    }
}

proptest! {
    /// `expr::parse` never panics on arbitrary input; it always returns a `Result`.
    #[test]
    fn expr_parse_never_panics(s in "\\PC{0,64}") {
        let _ = expr::parse(&s);
    }

    /// A finite numeric literal, formatted plainly and parsed back, evaluates
    /// to (very nearly) the original value.
    #[test]
    fn expr_round_trips_numeric_literals(n in -1.0e6f64..1.0e6f64) {
        let src = format!("{n}");
        let e = expr::parse(&src).expect("plain decimal literal must parse");
        let got = e.eval(&EmptyEnv).expect("a literal needs no variables");
        let tol = 1e-9 * n.abs().max(1.0);
        prop_assert!((got - n).abs() <= tol, "{src} -> {got}, want {n}");
    }
}

// ---------------------------------------------------------------------
// scenario::fill: always a distribution (sums to 1, non-negative).
// ---------------------------------------------------------------------

proptest! {
    #[test]
    fn fill_is_always_a_distribution(
        pattern in prop::collection::vec(prop::option::of(0.0f64..=3.0), 1..8),
        uniform in any::<bool>(),
    ) {
        let edges: Vec<usize> = (0..pattern.len()).collect();
        let mode = if uniform { ProbFill::Uniform } else { ProbFill::Residual };
        let dist = fill(&edges, &pattern, mode);
        prop_assert_eq!(dist.len(), edges.len());
        let sum: f64 = dist.iter().map(|&(_, p)| p).sum();
        prop_assert!((sum - 1.0).abs() <= 1e-9, "sum = {sum}");
        for &(_, p) in &dist {
            prop_assert!(p >= -1e-12, "negative probability {p}");
        }
    }
}

// ---------------------------------------------------------------------
// Random acyclic chance graphs: chain, mdp, sim agree.
// ---------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Absorption probabilities from a chain always sum to (almost) 1: every
    /// generated graph is a DAG, so every start absorbs somewhere.
    #[test]
    fn chain_absorption_sums_to_one(seed in any::<u64>(), n_states in 1usize..5, n_terms in 1usize..4) {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let rc = random_chain(&mut rng, n_states, n_terms);
        let v = view(&rc.graph, &Scenario::default());
        let sol = mdp::solve(&v, &mdp::SolveOptions::default()).expect("acyclic chance graphs always solve");
        let ms = vec![("dollars".to_string(), v.metric("dollars").unwrap())];
        let c = chain::chain(&v, &sol, v.start, &ms).expect("a DAG of chance nodes always absorbs");
        let total: f64 = c.absorption.iter().map(|&(_, p)| p).sum();
        prop_assert!((total - 1.0).abs() <= 1e-6, "n_states={} n_terms={} total={}", rc.n_states, rc.n_terms, total);
        for &(_, p) in &c.absorption {
            prop_assert!(p >= -1e-9);
        }
    }

    /// Risk-neutral, undiscounted: the MDP value at the start equals the
    /// chain's expected utility minus its expected cost, for the same policy.
    #[test]
    fn mdp_value_matches_chain_expectation(seed in any::<u64>(), n_states in 1usize..5, n_terms in 1usize..4) {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let rc = random_chain(&mut rng, n_states, n_terms);
        let v = view(&rc.graph, &Scenario::default());
        let sol = mdp::solve(&v, &mdp::SolveOptions::default()).unwrap();
        let ms = vec![("dollars".to_string(), v.metric("dollars").unwrap())];
        let c = chain::chain(&v, &sol, v.start, &ms).unwrap();
        let from_chain = c.expected_utility - c.expected["dollars"];
        let from_mdp = sol.value[v.start];
        let tol = 1e-6 * from_mdp.abs().max(from_chain.abs()).max(1.0);
        prop_assert!((from_mdp - from_chain).abs() <= tol, "mdp={from_mdp} chain={from_chain}");
    }

    /// Monte Carlo mean converges to the closed-form chain mean.
    #[test]
    fn simulation_mean_matches_chain(seed in any::<u64>(), n_states in 1usize..4, n_terms in 1usize..3) {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let rc = random_chain(&mut rng, n_states, n_terms);
        let v = view(&rc.graph, &Scenario::default());
        let sol = mdp::solve(&v, &mdp::SolveOptions::default()).unwrap();
        let ms = vec![("dollars".to_string(), v.metric("dollars").unwrap())];
        let c = chain::chain(&v, &sol, v.start, &ms).unwrap();
        let chain_mean = c.expected_utility - c.expected["dollars"];
        let sim = litgraph::algo::sim::simulate(
            &v,
            &sol,
            v.start,
            &ms,
            &litgraph::algo::sim::SimOptions {
                runs: 4000,
                seed: 42,
                alpha: 0.1,
                max_steps: 1000,
                sample_durations: false,
                keep_samples: 0,
            },
        )
        .unwrap();
        // 8-sigma bound on the sample mean: astronomically unlikely to
        // false-fail, still tight enough to catch a real divergence.
        let tol = 8.0 * sim.net.std / (sim.runs as f64).sqrt() + 1e-6;
        prop_assert!(
            (sim.net.mean - chain_mean).abs() <= tol,
            "sim={} chain={} tol={}",
            sim.net.mean,
            chain_mean,
            tol
        );
    }
}

// ---------------------------------------------------------------------
// Worst <= Expected <= Optimistic-mixed, and CARA(a -> 0) -> Expected.
// A single mixed node: we may take a fixed-payoff move, or nature may fire
// first with probability p to a (possibly worse, possibly better) payoff.
// ---------------------------------------------------------------------

fn mixed_node_graph(my_payoff: f64, nature_payoff: f64, p: f64) -> Graph {
    let nodes = vec![
        RawNode { id: "n0".into(), label: "n0".into(), ..Default::default() },
        RawNode {
            id: "good".into(),
            label: "good".into(),
            kind: Some(NodeKind::Terminal),
            payoff: Some(my_payoff),
            ..Default::default()
        },
        RawNode {
            id: "world".into(),
            label: "world".into(),
            kind: Some(NodeKind::Terminal),
            payoff: Some(nature_payoff),
            ..Default::default()
        },
    ];
    let edges = vec![
        RawEdge { from: "n0".into(), to: "good".into(), label: "choose".into(), actor: "applicant".into(), ..Default::default() },
        RawEdge { from: "n0".into(), to: "world".into(), label: "nature".into(), probability: Some(p), ..Default::default() },
    ];
    let pack = Pack {
        schema_version: 2,
        id: "m".into(),
        title: "mixed".into(),
        start_node_id: "n0".into(),
        nodes,
        edges,
        ..Default::default()
    };
    Graph::compile(&[pack], &LinkFile::default(), &CompileOptions { no_continuations: true }).unwrap()
}

proptest! {
    #[test]
    fn worst_le_expected_le_optimistic(
        my_payoff in -5_000.0f64..5_000.0,
        nature_payoff in -5_000.0f64..5_000.0,
        p in 0.0f64..=1.0,
    ) {
        let g = mixed_node_graph(my_payoff, nature_payoff, p);

        let worst = Scenario { objective: Objective::Worst, ..Default::default() };
        let expected = Scenario::default();
        let optimistic = Scenario { mixed: MixedMode::Optimistic, ..Default::default() };

        let solve = |sc: &Scenario| {
            let v = view(&g, sc);
            mdp::solve(&v, &mdp::SolveOptions::default()).unwrap().value[v.start]
        };
        let (vw, ve, vo) = (solve(&worst), solve(&expected), solve(&optimistic));
        prop_assert!(vw <= ve + 1e-9, "worst {vw} > expected {ve}");
        prop_assert!(ve <= vo + 1e-9, "expected {ve} > optimistic {vo}");
    }

    /// As the CARA risk-aversion coefficient shrinks to 0, its certainty
    /// equivalent converges to the plain (risk-neutral) expectation.
    #[test]
    fn cara_converges_to_expected_as_a_shrinks(
        my_payoff in -5_000.0f64..5_000.0,
        nature_payoff in -5_000.0f64..5_000.0,
        p in 0.0f64..=1.0,
    ) {
        let g = mixed_node_graph(my_payoff, nature_payoff, p);
        let expected = Scenario::default();
        let cara = Scenario { objective: Objective::Cara { a: 1e-8 }, ..Default::default() };
        let solve = |sc: &Scenario| {
            let v = view(&g, sc);
            mdp::solve(&v, &mdp::SolveOptions::default()).unwrap().value[v.start]
        };
        let (ve, vc) = (solve(&expected), solve(&cara));
        // Second-order Taylor remainder ~ 0.5 * a * Var(q); with a = 1e-8 and
        // |payoff| <= 5000 this is comfortably under 1.
        prop_assert!((ve - vc).abs() <= 1.0, "expected {ve} vs cara(a=1e-8) {vc}");
    }
}

// ---------------------------------------------------------------------
// Dijkstra <= every k-shortest path, and k-shortest totals non-decreasing.
// ---------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn dijkstra_le_k_shortest_and_k_shortest_nondecreasing(
        seed in any::<u64>(), n_states in 2usize..6, n_terms in 1usize..4,
    ) {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let rc = random_chain(&mut rng, n_states, n_terms);
        let v = view(&rc.graph, &Scenario::default());
        let w = v.metric("dollars").unwrap();
        for t in 0..rc.n_terms {
            let ti = rc.graph.node(&format!("t{t}")).unwrap();
            let Some(shortest) = paths::shortest(&v, v.start, ti, &w).unwrap() else { continue };
            let ks = paths::k_shortest(&v, v.start, ti, &w, 5).unwrap();
            prop_assert!(!ks.is_empty());
            prop_assert!((ks[0].totals[0] - shortest.totals[0]).abs() <= 1e-9);
            for pair in ks.windows(2) {
                prop_assert!(pair[0].totals[0] <= pair[1].totals[0] + 1e-9);
            }
            for p in &ks {
                prop_assert!(p.totals[0] >= shortest.totals[0] - 1e-9);
            }
        }
    }

    /// Every path the Pareto frontier returns is non-dominated by any other
    /// path it returns (that's the whole point of a frontier).
    #[test]
    fn pareto_frontier_is_non_dominated(seed in any::<u64>(), n_states in 2usize..6, n_terms in 1usize..3) {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let rc = random_chain(&mut rng, n_states, n_terms);
        let v = view(&rc.graph, &Scenario::default());
        let ws = vec![v.metric("dollars").unwrap(), v.metric("hours").unwrap()];
        for t in 0..rc.n_terms {
            let ti = rc.graph.node(&format!("t{t}")).unwrap();
            let fr = paths::pareto(&v, v.start, ti, &ws, 100_000).unwrap();
            for (i, a) in fr.paths.iter().enumerate() {
                for (j, b) in fr.paths.iter().enumerate() {
                    if i == j {
                        continue;
                    }
                    let dominates = a.totals.iter().zip(&b.totals).all(|(x, y)| *x <= y + 1e-9)
                        && a.totals.iter().zip(&b.totals).any(|(x, y)| *x < y - 1e-9);
                    prop_assert!(!dominates, "{:?} dominates {:?} in frontier for t{t}", a.totals, b.totals);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------
// min-cut == max-flow, checked against a brute-force max flow on tiny
// integer-capacity DAGs.
// ---------------------------------------------------------------------

/// Exhaustively maximizes flow out of node 0 over all integer per-edge flows
/// in `0..=cap`, subject to conservation at every node but 0 and `t`. Only
/// tractable because these test graphs are tiny (few edges, small caps).
fn brute_force_max_flow(n: usize, edges: &[(usize, usize, i32)], t: usize) -> i32 {
    let mut best = 0;
    let mut flow = vec![0i32; edges.len()];
    fn rec(i: usize, edges: &[(usize, usize, i32)], n: usize, t: usize, flow: &mut Vec<i32>, best: &mut i32) {
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
            *best = (*best).max(-balance[0] as i32);
            return;
        }
        for f in 0..=edges[i].2 {
            flow[i] = f;
            rec(i + 1, edges, n, t, flow, best);
        }
    }
    rec(0, edges, n, t, &mut flow, &mut best);
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
        let g = Graph::compile(&[pack], &LinkFile::default(), &CompileOptions { no_continuations: true }).unwrap();
        let v = view(&g, &Scenario::default());
        let cap = v.metric("hours").unwrap();
        let cut = structure::min_cut(&v, 0, t, &cap);
        prop_assert!((cut.value - f64::from(brute)).abs() < 1e-6, "mincut {} vs bruteforce maxflow {}", cut.value, brute);
    }
}

// ---------------------------------------------------------------------
// Dominators: removing idom(n) disconnects n from root.
// ---------------------------------------------------------------------

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn removing_idom_disconnects_node_from_root(seed in any::<u64>(), n_states in 2usize..6, n_terms in 1usize..4) {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let rc = random_chain(&mut rng, n_states, n_terms);
        let v = view(&rc.graph, &Scenario::default());
        let idom = structure::dominators(&v, v.start);
        for n in 0..v.g.nodes.len() {
            let Some(d) = idom[n] else { continue };
            if n == v.start {
                continue;
            }
            // BFS from start over active edges, skipping node `d` entirely.
            let mut seen = vec![false; v.g.nodes.len()];
            seen[v.start] = true;
            let mut stack = vec![v.start];
            while let Some(u) = stack.pop() {
                for e in v.outs(u) {
                    let w = v.g.edges[e].to;
                    if w == d || seen[w] {
                        continue;
                    }
                    seen[w] = true;
                    stack.push(w);
                }
            }
            prop_assert!(!seen[n], "node {n} still reachable from start without idom {d}");
        }
    }
}
