// SPDX-License-Identifier: GPL-3.0-or-later
//! Parity with the original TypeScript engine (civ-pro-the-gathering
//! src/lib/graph). Fixtures: tests/fixtures/ts-parity/<pack>.json.
//!
//! v1 semantics are reproduced by scenario modes, not special code paths:
//! `mixed: optimistic` + `prob_fill: uniform` for value iteration; the v1
//! absorbing chain used the *self-only* reading with the best self edge by
//! the optimistic Q*.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::cast_possible_truncation)]

use litgraph::algo::{chain, mdp, paths, structure};
use litgraph::model::{CompileOptions, Graph, LinkFile, Pack, Role};
use litgraph::scenario::{MixedMode, ProbFill, Scenario, View};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn packs() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(format!("{ROOT}/tests/fixtures/ts-parity"))
        .unwrap()
        .filter_map(std::result::Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .map(|p| p.file_stem().unwrap().to_string_lossy().to_string())
        .collect();
    v.sort();
    v
}

fn load(id: &str) -> (Graph, Value) {
    let pack =
        Pack::from_json(&std::fs::read_to_string(format!("{ROOT}/packs/{id}.json")).unwrap())
            .unwrap();
    let fx: Value = serde_json::from_str(
        &std::fs::read_to_string(format!("{ROOT}/tests/fixtures/ts-parity/{id}.json")).unwrap(),
    )
    .unwrap();
    (
        Graph::compile(
            &[pack],
            &LinkFile::default(),
            &CompileOptions {
                no_continuations: true,
            },
        )
        .unwrap(),
        fx,
    )
}

fn v1(rate: f64, mixed: MixedMode) -> Scenario {
    Scenario {
        params: BTreeMap::from([("rate".into(), rate)]),
        mixed,
        prob_fill: ProbFill::Uniform,
        ..Default::default()
    }
}

fn close(a: f64, b: f64, what: &str) {
    let tol = 1e-3 + 1e-6 * b.abs();
    assert!((a - b).abs() <= tol, "{what}: rust {a} vs ts {b}");
}

#[test]
fn value_iteration_matches() {
    for id in packs() {
        let (g, fx) = load(&id);
        for (rate, key) in [(500.0, "valueIteration500"), (250.0, "valueIteration250")] {
            let v = View::new(&g, &v1(rate, MixedMode::Optimistic)).unwrap();
            let sol = mdp::solve(&v, &mdp::SolveOptions::default()).unwrap();
            for (node, want) in fx[key]["value"].as_object().unwrap() {
                let n = g.node(node).unwrap();
                close(
                    sol.value[n],
                    want.as_f64().unwrap(),
                    &format!("{id} V({node}) @ {rate}"),
                );
            }
            // Per-edge Q (the TS qValue map collides on parallel edges; edgeQ does not).
            for row in fx[key]["edgeQ"].as_array().unwrap() {
                let (from, to, label) = (
                    row["from"].as_str().unwrap(),
                    row["to"].as_str().unwrap(),
                    row["label"].as_str().unwrap(),
                );
                let f = g.node(from).unwrap();
                let e = g.out[f]
                    .iter()
                    .copied()
                    .find(|&e| g.edges[e].label == label && g.nodes[g.edges[e].to].local_id == to)
                    .unwrap();
                if g.nodes[f].is_terminal() {
                    continue; // v1 computed Q out of terminals; they are absorbing and never used
                }
                close(
                    sol.q[e],
                    row["q"].as_f64().unwrap(),
                    &format!("{id} Q({from}→{to} `{label}`)"),
                );
            }
        }
    }
}

#[test]
fn absorbing_chain_matches() {
    for id in packs() {
        let (g, fx) = load(&id);
        let ac = &fx["absorbingChain500"];
        if !ac["ok"].as_bool().unwrap_or(false) {
            continue;
        }
        // Replay the TS engine's exact policy (it breaks Q ties by edge order,
        // which is arbitrary on packs with no payoffs/costs), then run a
        // self-only chain. This isolates the chain math from tie-breaking.
        let vs = View::new(&g, &v1(500.0, MixedMode::SelfOnly)).unwrap();
        let mut sol = mdp::solve(&vs, &mdp::SolveOptions::default()).unwrap();
        sol.choice = BTreeMap::new();
        for (node, e) in ac["policy"].as_object().unwrap() {
            let n = g.node(node).unwrap();
            let (to, label) = (e["to"].as_str().unwrap(), e["label"].as_str().unwrap());
            let edge = g.out[n]
                .iter()
                .copied()
                .find(|&x| g.edges[x].label == label && g.nodes[g.edges[x].to].local_id == to)
                .unwrap();
            assert_eq!(vs.role[edge], Role::Me);
            sol.choice.insert(n, edge);
        }
        let ms = vec![
            ("dollars".to_string(), vs.metric("dollars").unwrap()),
            ("days".to_string(), vs.metric("days").unwrap()),
        ];
        let c = chain::chain(&vs, &sol, vs.start, &ms).unwrap_or_else(|e| panic!("{id}: {e}"));
        for (t, want) in ac["absorptionProbabilityAtStart"].as_object().unwrap() {
            let ti = g.node(t).unwrap();
            let got = c
                .absorption
                .iter()
                .find(|x| x.0 == ti)
                .map(|x| x.1)
                .unwrap_or(0.0);
            close(got, want.as_f64().unwrap(), &format!("{id} P(absorb {t})"));
        }
        close(
            c.expected["dollars"],
            ac["expectedCostAtStart"].as_f64().unwrap(),
            &format!("{id} E[cost]"),
        );
        close(
            c.expected["days"],
            ac["expectedDaysAtStart"].as_f64().unwrap(),
            &format!("{id} E[days]"),
        );
        close(
            c.expected_steps,
            ac["expectedStepsAtStart"].as_f64().unwrap(),
            &format!("{id} E[steps]"),
        );
    }
}

#[test]
fn shortest_paths_match() {
    for id in packs() {
        let (g, fx) = load(&id);
        let v = View::new(&g, &v1(500.0, MixedMode::Optimistic)).unwrap();
        for (key, metric) in [
            ("cheapestByDollars", "dollars"),
            ("fastestByDays", "days"),
            ("leastHours", "hours"),
        ] {
            let w = v.metric(metric).unwrap();
            for (t, want) in fx["dijkstra500"][key].as_object().unwrap() {
                let p = paths::shortest(&v, v.start, g.node(t).unwrap(), &w).unwrap();
                match (p, want.is_null()) {
                    (None, true) => {}
                    (Some(p), false) => close(
                        p.totals[0],
                        want["total"].as_f64().unwrap(),
                        &format!("{id} {key} → {t}"),
                    ),
                    (p, _) => panic!(
                        "{id} {key} → {t}: reachability differs (rust {:?})",
                        p.map(|p| p.totals)
                    ),
                }
            }
        }
    }
}

#[test]
fn min_cut_and_dominators_match() {
    for id in packs() {
        let (g, fx) = load(&id);
        let v = View::new(&g, &v1(500.0, MixedMode::Optimistic)).unwrap();
        let ones = vec![1.0; g.edges.len()];
        for (t, want) in fx["minCut500"].as_object().unwrap() {
            let c = structure::min_cut(&v, v.start, g.node(t).unwrap(), &ones);
            close(
                c.value,
                want["value"].as_f64().unwrap(),
                &format!("{id} mincut → {t}"),
            );
        }
        let idom = structure::dominators(&v, v.start);
        for (n, want) in fx["dominators"]["idom"].as_object().unwrap() {
            let ni = g.node(n).unwrap();
            let got = idom[ni].map(|d| g.nodes[d].local_id.clone());
            let want = want.as_str().map(String::from);
            // TS reports the root as its own idom; we report None.
            if ni == v.start {
                continue;
            }
            assert_eq!(got, want, "{id} idom({n})");
        }
    }
}

#[test]
fn cyclic_components_match() {
    for id in packs() {
        let (g, fx) = load(&id);
        let v = View::new(&g, &v1(500.0, MixedMode::Optimistic)).unwrap();
        let ours: BTreeSet<BTreeSet<String>> = structure::scc(&v)
            .into_iter()
            .filter(|c| structure::is_cyclic(&v, c))
            .map(|c| c.iter().map(|&n| g.nodes[n].local_id.clone()).collect())
            .collect();
        let theirs: BTreeSet<BTreeSet<String>> = fx["scc"]["cyclicComponents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                let members = c.get("members").or(c.get("nodes")).unwrap_or(c);
                members
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_str().unwrap().to_string())
                    .collect()
            })
            .collect();
        assert_eq!(ours, theirs, "{id} cyclic SCCs");
    }
}

#[test]
fn pareto_frontier_matches() {
    for id in packs() {
        let (g, fx) = load(&id);
        let v = View::new(&g, &v1(500.0, MixedMode::Optimistic)).unwrap();
        let ws = vec![v.metric("days").unwrap(), v.metric("dollars").unwrap()];
        for (t, want) in fx["paretoPaths500"].as_object().unwrap() {
            if want["truncated"].as_bool().unwrap_or(true) {
                continue; // v1 enumeration was capped; its frontier is not ground truth
            }
            let fr = paths::pareto(&v, v.start, g.node(t).unwrap(), &ws, 2_000_000).unwrap();
            let ours: BTreeSet<(i64, i64)> = fr
                .paths
                .iter()
                .map(|p| {
                    (
                        (p.totals[0] * 1000.0).round() as i64,
                        (p.totals[1] * 1000.0).round() as i64,
                    )
                })
                .collect();
            let theirs: BTreeSet<(i64, i64)> = want["frontier"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| {
                    (
                        (p["totalDays"].as_f64().unwrap() * 1000.0).round() as i64,
                        (p["totalCost"].as_f64().unwrap() * 1000.0).round() as i64,
                    )
                })
                .collect();
            assert_eq!(ours, theirs, "{id} pareto → {t}");
        }
    }
}
