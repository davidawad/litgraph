// SPDX-License-Identifier: GPL-3.0-or-later
//! Property tests for probability uncertainty on random small acyclic
//! decision graphs (our choices and chance nodes mixed): the conjugate
//! posterior mean, robust value ≤ nominal (with radius 0 exactly nominal),
//! information values are non-negative and ordered, and all of them vanish
//! when a dominant option makes the decision unchangeable.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use litgraph::algo::mdp;
use litgraph::algo::voi::{self, Study, VoiOptions};
use litgraph::model::{CompileOptions, Graph, LinkFile, NodeKind, Pack, RawEdge, RawNode};
use litgraph::scenario::{Objective, Scenario, View};
use proptest::prelude::*;
use rand::Rng;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

/// States `s0..s{n-1}` (each a choice of ours or a chance node with authored
/// probabilities) over terminals `t0..t{m-1}`; edges only go to later states
/// or terminals. With `dominant`, the start also offers a sure payoff above
/// every terminal.
fn random_graph(seed: u64, dominant: bool) -> Graph {
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let (n, m) = (rng.gen_range(2..=6), rng.gen_range(2..=4));
    let mut nodes: Vec<RawNode> = (0..n)
        .map(|i| RawNode {
            id: format!("s{i}"),
            label: format!("s{i}"),
            ..Default::default()
        })
        .collect();
    for t in 0..m {
        nodes.push(RawNode {
            id: format!("t{t}"),
            label: format!("t{t}"),
            kind: Some(NodeKind::Terminal),
            payoff: Some(rng.gen_range(-1000.0..=1000.0)),
            ..Default::default()
        });
    }
    let mut edges = vec![];
    for i in 0..n {
        let mut pool: Vec<String> = (i + 1..n).map(|j| format!("s{j}")).collect();
        pool.extend((0..m).map(|t| format!("t{t}")));
        let k = rng.gen_range(2..=3.min(pool.len()));
        let chance = rng.gen_bool(0.5);
        let w: Vec<f64> = (0..k).map(|_| rng.gen_range(1.0..=5.0)).collect();
        let sum: f64 = w.iter().sum();
        for wi in w {
            let to = pool.remove(rng.gen_range(0..pool.len()));
            edges.push(RawEdge {
                from: format!("s{i}"),
                to,
                label: "e".into(),
                actor: if chance { "examiner" } else { "applicant" }.into(),
                probability: chance.then_some(wi / sum),
                cost: Some(rng.gen_range(0.0..=50.0)),
                ..Default::default()
            });
        }
    }
    if dominant {
        nodes.push(RawNode {
            id: "sure".into(),
            label: "sure".into(),
            kind: Some(NodeKind::Terminal),
            payoff: Some(1e6),
            ..Default::default()
        });
        edges.push(RawEdge {
            from: "s0".into(),
            to: "sure".into(),
            label: "sure".into(),
            actor: "applicant".into(),
            ..Default::default()
        });
    }
    let pack = Pack {
        schema_version: 2,
        id: "r".into(),
        title: "random".into(),
        start_node_id: "s0".into(),
        nodes,
        edges,
        ..Default::default()
    };
    Graph::compile(&[pack], &LinkFile::default(), &CompileOptions::default()).unwrap()
}

fn solve_value(g: &Graph, sc: &Scenario) -> Vec<f64> {
    let v = View::new(g, sc).unwrap();
    mdp::solve(&v, &mdp::SolveOptions::default()).unwrap().value
}

fn robust(credibility: f64, radius: Option<f64>, concentration: f64) -> Scenario {
    let mut sc = Scenario {
        objective: Objective::Robust {
            credibility,
            radius,
            samples: 300,
            seed: 1,
        },
        ..Default::default()
    };
    sc.uncertainty.default_concentration = Some(concentration);
    sc
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// The ambiguity set contains the posterior mean, so the worst case over
    /// it can't beat the nominal value, at any node; radius 0 is nominal.
    #[test]
    fn robust_value_never_exceeds_nominal(
        seed in any::<u64>(),
        credibility in 0.05f64..0.99,
        concentration in 0.5f64..500.0,
    ) {
        let g = random_graph(seed, false);
        let nominal = solve_value(&g, &Scenario::default());
        let rob = solve_value(&g, &robust(credibility, None, concentration));
        for (r, n) in rob.iter().zip(&nominal) {
            prop_assert!(*r <= n + 1e-9 * n.abs().max(1.0), "robust {r} > nominal {n}");
        }
        prop_assert_eq!(solve_value(&g, &robust(0.5, Some(0.0), 1.0)), nominal);
    }

    /// Posterior mean = (c·p + counts) / (c + Σ counts) at every observed node.
    #[test]
    fn posterior_mean_is_the_conjugate_closed_form(
        seed in any::<u64>(),
        c in 0.5f64..100.0,
        counts in proptest::collection::vec(0u32..20, 3),
    ) {
        let g = random_graph(seed, false);
        let prior = View::new(&g, &Scenario::default()).unwrap();
        let Some(&gi) = prior.belief.uncertain().first() else { return Ok(()); };
        let grp = &prior.belief.groups[gi];
        let node = g.nodes[grp.node].id.clone();
        let mut sc = Scenario::default();
        sc.uncertainty.default_concentration = Some(c);
        let obs = sc.observe.entry(node).or_default();
        let ks: Vec<f64> = grp.edges.iter().zip(&counts).map(|(_, &k)| f64::from(k)).collect();
        for (&e, &k) in grp.edges.iter().zip(&ks) {
            obs.insert(g.edges[e].id.clone(), k);
        }
        let post = View::new(&g, &sc).unwrap();
        let total: f64 = ks.iter().sum();
        for (i, &e) in grp.edges.iter().enumerate() {
            let expect = (c * grp.prior_mean[i] + ks[i]) / (c + total);
            prop_assert!((post.prob[e].unwrap() - expect).abs() < 1e-12);
        }
    }

    /// Every information value is ≥ 0, EVPI(outcome) ≥ EVPPI ≥ EVSI(k) (up to
    /// Monte Carlo error) at an acyclic node, and all are exactly 0 once a
    /// dominant option makes every other one irrelevant.
    #[test]
    fn information_values_are_ordered_and_vanish_when_moot(
        seed in any::<u64>(),
        k in 1u64..30,
        dominant in any::<bool>(),
    ) {
        let g = random_graph(seed, dominant);
        let v = View::new(&g, &Scenario::default()).unwrap();
        let studies: Vec<Study> = v.belief.uncertain().into_iter().map(|group| Study { group, k }).collect();
        let o = VoiOptions { samples: 150, seed, top: 20, max_nodes: 50 };
        let out = voi::voi(&v, v.start, &studies, &o).unwrap();
        prop_assert!(out.evpi_total.value >= 0.0);
        for row in &out.nodes {
            let evpi = row.evpi_outcome.unwrap();
            let evppi = row.evppi.unwrap();
            prop_assert!(evpi >= 0.0 && evppi.value >= 0.0);
            prop_assert!(evppi.value <= evpi + 5.0 * evppi.std_error + 1e-6, "EVPPI {evppi:?} > EVPI {evpi}");
            let st = studies.iter().position(|s| s.group == row.group).unwrap();
            let evsi = out.studies[st];
            prop_assert!(evsi.value >= 0.0);
            prop_assert!(evsi.value <= evpi + 5.0 * evsi.std_error + 1e-6, "EVSI {evsi:?} > EVPI {evpi}");
            if dominant {
                prop_assert_eq!(evpi, 0.0);
                prop_assert_eq!(evppi.value, 0.0);
                prop_assert_eq!(evsi.value, 0.0);
            }
        }
        if dominant {
            prop_assert_eq!(out.evpi_total.value, 0.0);
        }
    }
}
