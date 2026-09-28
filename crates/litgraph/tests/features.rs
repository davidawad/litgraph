// SPDX-License-Identifier: GPL-3.0-or-later
//! Semantics of the new (non-v1) capabilities on small hand-built graphs
//! where the right answer can be computed by hand.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use litgraph::algo::{chain, mdp, paths, sim, sweep};
use litgraph::model::{CompileOptions, Graph, LinkFile, Pack};
use litgraph::scenario::{FeeShift, Objective, OpponentMode, Scenario, View};
use serde_json::json;
use std::collections::BTreeMap;

/// start --settle(self, 10h)--> settled (+400k)
/// start --litigate(self, 100h)--> trial (chance)
/// trial --win p=.6--> won (+1M, fee-eligible)   trial --lose p=.4--> lost (0)
fn toy() -> Graph {
    let pack: Pack = serde_json::from_value(json!({
        "schemaVersion": 2, "id": "toy", "title": "toy", "startNodeId": "start",
        "roles": { "applicant": "self", "examiner": "opponent", "office": "nature" },
        "nodes": [
            { "id": "start", "kind": "decision", "label": "Settle or litigate" },
            { "id": "trial", "kind": "state", "label": "Trial" },
            { "id": "settled", "kind": "terminal", "label": "Settled", "payoff": 400000, "outcome": ["settlement"] },
            { "id": "won", "kind": "terminal", "label": "Won", "payoff": 1000000, "outcome": ["win", "fee-eligible"] },
            { "id": "lost", "kind": "terminal", "label": "Lost", "payoff": 0, "outcome": ["loss"] }
        ],
        "edges": [
            { "id": "settle", "from": "start", "to": "settled", "label": "Settle", "actor": "applicant", "hours": 10, "tags": ["settlement"] },
            { "id": "litigate", "from": "start", "to": "trial", "label": "Litigate", "actor": "applicant", "hours": 100, "cost": 5000,
              "duration": { "min": 300, "mode": 365, "max": 700 } },
            { "id": "win", "from": "trial", "to": "won", "label": "Win", "actor": "office", "probability": 0.6 },
            { "id": "lose", "from": "trial", "to": "lost", "label": "Lose", "actor": "office", "probability": 0.4 }
        ]
    }))
    .unwrap();
    Graph::compile(&[pack], &LinkFile::default(), &CompileOptions::default()).unwrap()
}

fn sc(v: serde_json::Value) -> Scenario {
    serde_json::from_value(v).unwrap()
}

fn solve<'g>(g: &'g Graph, s: &Scenario) -> (View<'g>, mdp::Solution) {
    let v = View::new(g, s).unwrap();
    let sol = mdp::solve(&v, &mdp::SolveOptions::default()).unwrap();
    (v, sol)
}

#[test]
fn risk_neutral_litigates() {
    let g = toy();
    let (v, sol) = solve(&g, &Scenario::default());
    // litigate: -(100*500+5000) + 0.6*1e6 = 545,000 > settle: -5000 + 400,000
    assert!((sol.value[v.start] - 545_000.0).abs() < 1e-6);
    assert_eq!(g.edges[sol.choice[&v.start]].label, "Litigate");
}

#[test]
fn cara_risk_aversion_settles() {
    let g = toy();
    let (v, sol) = solve(
        &g,
        &sc(json!({ "objective": { "type": "cara", "a": 5e-6 } })),
    );
    assert_eq!(g.edges[sol.choice[&v.start]].label, "Settle");
    // CE of the gamble < its mean.
    let q_lit = sol.q[g.edge("litigate").unwrap()];
    assert!(q_lit < 545_000.0);
}

#[test]
fn custom_cost_and_utility_expressions() {
    let g = toy();
    // Price attorney time at 2,000/h via a custom metric and make losses hurt 3x.
    let s = sc(json!({
        "metrics": { "biglaw": "hours * 2000 + fees" },
        "cost": "biglaw",
        "payoffs": { "lost": -100000 },
        "utility": "payoff < 0 ? 3 * payoff : payoff"
    }));
    let (v, sol) = solve(&g, &s);
    let want_lit = -(100.0 * 2000.0 + 5000.0) + 0.6 * 1e6 + 0.4 * -300_000.0;
    assert!((sol.q[g.edge("litigate").unwrap()] - want_lit).abs() < 1e-6);
    assert!((sol.value[v.start] - (-20_000.0 + 400_000.0_f64).max(want_lit)).abs() < 1e-6);
}

#[test]
fn fee_shift_is_exact_under_policy() {
    let g = toy();
    let s = Scenario {
        fee_shift: Some(FeeShift {
            fraction: 0.5,
            eligible: None,
        }),
        ..Default::default()
    };
    let (_, sol) = solve(&g, &s);
    // litigate cost 55,000 recovered at 50% with P(win)=.6 → 55,000*(1-.3) = 38,500
    let e = g.edge("litigate").unwrap();
    assert!((sol.cost[e] - 38_500.0).abs() < 1e-6);
}

#[test]
fn discounting_uses_elapsed() {
    let g = toy();
    let (_, sol) = solve(&g, &sc(json!({ "discount_annual": 0.10 })));
    let q = sol.q[g.edge("litigate").unwrap()];
    let want = -55_000.0 + (1.1f64).powf(-1.0) * 600_000.0;
    assert!((q - want).abs() < 1e-6);
}

#[test]
fn mask_and_probability_override() {
    let g = toy();
    let (v, sol) = solve(
        &g,
        &sc(json!({ "mask": "!tag('settlement')", "probabilities": { "win": 0.2 } })),
    );
    // Settlement removed; P(win) = .2 → -55,000 + 200,000.
    assert!((sol.value[v.start] - 145_000.0).abs() < 1e-6);
}

/// Regression: at a mixed node whose world edge has no probability, the old
/// self-only fallback deleted the way out and forced a costly self-loop
/// forever (CoFC discovery-open: V → −$1.27B). Act-or-wait lets us wait.
#[test]
fn act_or_wait_keeps_the_world_exit() {
    let pack: Pack = serde_json::from_value(json!({
        "schemaVersion": 2, "id": "aw", "title": "aw", "startNodeId": "disc",
        "roles": { "applicant": "self", "office": "nature" },
        "nodes": [
            { "id": "disc", "kind": "state", "label": "Discovery open" },
            { "id": "done", "kind": "terminal", "label": "Discovery closed", "payoff": 1000 }
        ],
        "edges": [
            { "id": "compel", "from": "disc", "to": "disc", "label": "Move to compel", "actor": "applicant", "hours": 10 },
            { "id": "cutoff", "from": "disc", "to": "done", "label": "Discovery proceeds to cutoff", "actor": "office" }
        ]
    }))
    .unwrap();
    let g = Graph::compile(&[pack], &LinkFile::default(), &CompileOptions::default()).unwrap();
    let (v, sol) = solve(&g, &Scenario::default());
    assert!(sol.converged);
    assert_eq!(sol.value[v.start], 1000.0);
    assert_eq!(sol.choice[&v.start], litgraph::scenario::WAIT);
    let ms = vec![("dollars".to_string(), v.metric("dollars").unwrap())];
    let c = chain::chain(&v, &sol, v.start, &ms).unwrap();
    assert_eq!(c.expected["dollars"], 0.0);
}

#[test]
fn probability_fn_rewrites_and_renormalizes() {
    let g = toy();
    // A plaintiff-friendly forum: wins 1.5x as likely before renormalization → .9/(.9+.4)
    let (v, _) = solve(
        &g,
        &sc(json!({ "probability_fn": "label_has('win') ? p * 1.5 : p" })),
    );
    let pw = v.prob[g.edge("win").unwrap()].unwrap();
    assert!((pw - 0.9 / 1.3).abs() < 1e-12);
    assert!(v
        .warnings
        .iter()
        .any(|w| w.code == "probability-renormalized"));
}

#[test]
fn chain_and_simulation_agree() {
    let g = toy();
    let (v, sol) = solve(&g, &Scenario::default());
    let ms = vec![("dollars".to_string(), v.metric("dollars").unwrap())];
    let c = chain::chain(&v, &sol, v.start, &ms).unwrap();
    assert!((c.expected["dollars"] - 55_000.0).abs() < 1e-9);
    let won = g.node("won").unwrap();
    assert!((c.absorption.iter().find(|x| x.0 == won).unwrap().1 - 0.6).abs() < 1e-12);
    let r = sim::simulate(
        &v,
        &sol,
        v.start,
        &ms,
        &sim::SimOptions {
            runs: 20_000,
            seed: 1,
            alpha: 0.1,
            max_steps: 100,
            sample_durations: false,
            keep_samples: 0,
        },
    )
    .unwrap();
    assert!((r.net.mean - 545_000.0).abs() < 15_000.0);
    assert!((r.p_loss - 0.4).abs() < 0.02); // losing nets −55k
    assert!((r.cvar + 55_000.0).abs() < 1e-6);
}

#[test]
fn sweep_finds_rate_breakpoint() {
    let g = toy();
    // Litigate beats settle while 90h*rate + 5000 < 200,000  ⇔ rate < 2166.67
    let spec = sweep::SweepSpec { param: "rate", lo: 500.0, hi: 5000.0, steps: 10, watch: &[], tol: 1e-3 };
    let r = sweep::sweep(&g, &Scenario::default(), &spec).unwrap();
    assert_eq!(r.breakpoints.len(), 1);
    assert!((r.breakpoints[0].at - 195_000.0 / 90.0).abs() < 1e-2);
}

#[test]
fn adversarial_opponent_minimizes() {
    let pack: Pack = serde_json::from_value(json!({
        "schemaVersion": 2, "id": "adv", "title": "adv", "startNodeId": "s",
        "roles": { "applicant": "self", "examiner": "opponent" },
        "nodes": [
            { "id": "s", "kind": "state", "label": "s" },
            { "id": "good", "kind": "terminal", "label": "good", "payoff": 10 },
            { "id": "bad", "kind": "terminal", "label": "bad", "payoff": -10 }
        ],
        "edges": [
            { "id": "a", "from": "s", "to": "good", "label": "concede", "actor": "examiner" },
            { "id": "b", "from": "s", "to": "bad", "label": "fight", "actor": "examiner" }
        ]
    }))
    .unwrap();
    let g = Graph::compile(&[pack], &LinkFile::default(), &CompileOptions::default()).unwrap();
    let (v, sol) = solve(&g, &Scenario::default());
    assert_eq!(sol.value[v.start], -10.0);
    let (v, sol) = solve(
        &g,
        &Scenario {
            opponent: OpponentMode::Chance,
            ..Default::default()
        },
    );
    assert_eq!(sol.value[v.start], 0.0);
    let (v, sol) = solve(
        &g,
        &Scenario {
            objective: Objective::Worst,
            opponent: OpponentMode::Chance,
            ..Default::default()
        },
    );
    assert_eq!(sol.value[v.start], -10.0);
}

#[test]
fn pareto_three_objectives_and_k_shortest() {
    let g = toy();
    let v = View::new(&g, &Scenario::default()).unwrap();
    let ws = vec![
        v.metric("dollars").unwrap(),
        v.metric("elapsed").unwrap(),
        v.metric("surprise").unwrap(),
    ];
    let fr = paths::pareto(&v, v.start, g.node("won").unwrap(), &ws, 1000).unwrap();
    assert_eq!(fr.paths.len(), 1);
    assert!((fr.paths[0].probability - 0.6).abs() < 1e-12);
    let w = v.metric("steps").unwrap();
    let ks = paths::k_shortest(&v, v.start, g.node("won").unwrap(), &w, 3).unwrap();
    assert_eq!(ks.len(), 1);
}

#[test]
fn unknown_variable_is_a_clear_error() {
    let g = toy();
    let err = View::new(&g, &sc(json!({ "cost": "hours * bogus" })))
        .err()
        .unwrap();
    assert!(err.to_string().contains("bogus"), "{err}");
    let _ = BTreeMap::<String, f64>::new();
}
