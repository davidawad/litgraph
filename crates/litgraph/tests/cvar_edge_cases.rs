// SPDX-License-Identifier: GPL-3.0-or-later
//! `Objective::Cvar` (`algo::cvar`) edge cases split out of `tests/cvar.rs`
//! (brute-force/proptest correctness verification) to keep each file under
//! the line cap: input validation, the combination warnings, and the
//! `backup` branches the correctness graphs don't reach — a forced
//! (`scenario.policy`) choice, an act-or-wait node where waiting is optimal,
//! a tie between two equal-value choices, an isolated sink, a genuine cycle,
//! and an adversarial opponent.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use litgraph::algo::cvar;
use litgraph::model::{CompileOptions, Graph, LinkFile, Pack};
use litgraph::scenario::{MixedMode, Objective, Scenario, View, WAIT};
use serde_json::json;

fn pack(j: serde_json::Value) -> Pack {
    serde_json::from_value(j).expect("test pack literal is well-formed")
}

fn compile(p: Pack) -> Graph {
    Graph::compile(&[p], &LinkFile::default(), &CompileOptions::default()).expect("compiles")
}

fn risky_vs_safe_graph() -> Graph {
    compile(pack(json!({
        "schemaVersion": 2, "id": "cv", "title": "cv", "startNodeId": "start",
        "nodes": [
            { "id": "start", "kind": "state", "label": "start" },
            { "id": "chance", "kind": "state", "label": "chance" },
            { "id": "good", "kind": "terminal", "label": "good", "payoff": 150.0 },
            { "id": "bad", "kind": "terminal", "label": "bad", "payoff": -1000.0 },
            { "id": "safe", "kind": "terminal", "label": "safe", "payoff": 20.0 }
        ],
        "edges": [
            { "id": "risky", "from": "start", "to": "chance", "label": "risky", "actor": "applicant" },
            { "id": "safe-edge", "from": "start", "to": "safe", "label": "safe", "actor": "applicant" },
            { "id": "to-good", "from": "chance", "to": "good", "label": "good", "actor": "either", "probability": 0.9 },
            { "id": "to-bad", "from": "chance", "to": "bad", "label": "bad", "actor": "either", "probability": 0.1 }
        ]
    })))
}

#[test]
fn cvar_rejects_alpha_out_of_range() {
    let g = risky_vs_safe_graph();
    let sc = Scenario::default();
    let v = View::new(&g, &sc).unwrap();
    assert!(cvar::solve(
        &v,
        0.0,
        41,
        None,
        &litgraph::algo::mdp::SolveOptions::default()
    )
    .is_err());
    assert!(cvar::solve(
        &v,
        1.5,
        41,
        None,
        &litgraph::algo::mdp::SolveOptions::default()
    )
    .is_err());
}

#[test]
fn cvar_warns_when_combined_with_discount_fee_shift_or_opponent_objective() {
    let g = risky_vs_safe_graph();
    let sc = Scenario {
        objective: Objective::Cvar {
            alpha: 0.2,
            grid: 21,
            y_lo: None,
            y_hi: None,
        },
        discount_annual: Some(0.05),
        fee_shift: Some(litgraph::scenario::FeeShift {
            fraction: 0.1,
            eligible: None,
        }),
        opponent_objective: Some("payoff".to_string()),
        ..Default::default()
    };
    let v = View::new(&g, &sc).unwrap();
    for code in [
        "cvar-ignores-discount",
        "cvar-ignores-fee-shift",
        "cvar-ignores-opponent-objective",
    ] {
        assert!(
            v.warnings.iter().any(|w| w.code == code),
            "missing warning {code}: {:?}",
            v.warnings
        );
    }
}

/// A pack exercising the `backup` branches the correctness graphs don't
/// reach: a forced (`scenario.policy`) choice, an act-or-wait node where
/// waiting is optimal, a tie between two equal-value choices, and an
/// isolated sink (every node's backup runs regardless of reachability from
/// `start`, since `algo::structure::scc` decomposes the whole graph).
fn coverage_graph() -> Graph {
    compile(pack(json!({
        "schemaVersion": 2, "id": "cvcov", "title": "cvcov", "startNodeId": "start",
        "roles": { "applicant": "self", "examiner": "opponent" },
        "nodes": [
            { "id": "start", "kind": "state", "label": "start" },
            { "id": "term-a", "kind": "terminal", "label": "term-a", "payoff": 100.0 },
            { "id": "term-b", "kind": "terminal", "label": "term-b", "payoff": -100.0 },
            { "id": "mid", "kind": "state", "label": "mid" },
            { "id": "act-term", "kind": "terminal", "label": "act-term", "payoff": -500.0 },
            { "id": "wait-term", "kind": "terminal", "label": "wait-term", "payoff": 300.0 },
            { "id": "tie-node", "kind": "state", "label": "tie-node" },
            { "id": "tie-a", "kind": "terminal", "label": "tie-a", "payoff": 50.0 },
            { "id": "tie-b", "kind": "terminal", "label": "tie-b", "payoff": 50.0 },
            { "id": "cyc-a", "kind": "state", "label": "cyc-a" },
            { "id": "cyc-b", "kind": "state", "label": "cyc-b" },
            { "id": "cyc-end", "kind": "terminal", "label": "cyc-end", "payoff": 15.0 },
            { "id": "dead", "kind": "state", "label": "dead" }
        ],
        "edges": [
            { "id": "to-mid", "from": "start", "to": "mid", "label": "to-mid", "actor": "applicant" },
            { "id": "to-a", "from": "start", "to": "term-a", "label": "to-a", "actor": "applicant" },
            { "id": "to-b", "from": "start", "to": "term-b", "label": "to-b", "actor": "applicant" },
            { "id": "act", "from": "mid", "to": "act-term", "label": "act", "actor": "applicant" },
            { "id": "world", "from": "mid", "to": "wait-term", "label": "world", "actor": "either" },
            { "id": "to-tie-a", "from": "tie-node", "to": "tie-a", "label": "to-tie-a", "actor": "applicant" },
            { "id": "to-tie-b", "from": "tie-node", "to": "tie-b", "label": "to-tie-b", "actor": "applicant" },
            { "id": "cyc-a-b", "from": "cyc-a", "to": "cyc-b", "label": "spin", "actor": "either", "cost": 1.0 },
            { "id": "cyc-b-a", "from": "cyc-b", "to": "cyc-a", "label": "spin-back", "actor": "either", "cost": 1.0 },
            { "id": "cyc-a-end", "from": "cyc-a", "to": "cyc-end", "label": "exit", "actor": "either" }
        ]
    })))
}

/// `start` is itself an act-or-wait node, forced (`scenario.policy`) to the
/// *world* edge's own id — `plan.rs` and this solver both read that as
/// "wait", not "take that edge as ours" (a distinct branch from forcing an
/// ordinary edge).
fn forced_to_wait_graph() -> Graph {
    compile(pack(json!({
        "schemaVersion": 2, "id": "cvforce", "title": "cvforce", "startNodeId": "start",
        "nodes": [
            { "id": "start", "kind": "state", "label": "start" },
            { "id": "act-term", "kind": "terminal", "label": "act-term", "payoff": 10.0 },
            { "id": "wait-term", "kind": "terminal", "label": "wait-term", "payoff": 20.0 }
        ],
        "edges": [
            { "id": "act", "from": "start", "to": "act-term", "label": "act", "actor": "applicant" },
            { "id": "world", "from": "start", "to": "wait-term", "label": "world", "actor": "either" }
        ]
    })))
}

#[test]
fn cvar_forced_to_the_world_edge_id_means_wait() {
    let g = forced_to_wait_graph();
    let mut policy = std::collections::BTreeMap::new();
    policy.insert("cvforce::start".to_string(), "cvforce::world".to_string());
    let sc = Scenario {
        objective: Objective::Cvar {
            alpha: 0.5,
            grid: 21,
            y_lo: None,
            y_hi: None,
        },
        mixed: MixedMode::ActOrWait,
        policy,
        ..Default::default()
    };
    let v = View::new(&g, &sc).unwrap();
    let cv = cvar::solve(
        &v,
        0.5,
        21,
        None,
        &litgraph::algo::mdp::SolveOptions::default(),
    )
    .unwrap();
    assert_eq!(cv.solution.choice[&v.start], WAIT);
}

/// An opponent-controlled fork under `Objective::Cvar` (which ignores
/// `opponent_objective` and always models the opponent adversarially):
/// the opponent picks the edge worse for us.
fn opponent_fork_graph() -> Graph {
    compile(pack(json!({
        "schemaVersion": 2, "id": "cvopp", "title": "cvopp", "startNodeId": "start",
        "roles": { "applicant": "self", "examiner": "opponent" },
        "nodes": [
            { "id": "start", "kind": "state", "label": "start" },
            { "id": "opp-lo", "kind": "terminal", "label": "opp-lo", "payoff": 10.0 },
            { "id": "opp-hi", "kind": "terminal", "label": "opp-hi", "payoff": 90.0 }
        ],
        "edges": [
            { "id": "to-opp-lo", "from": "start", "to": "opp-lo", "label": "to-opp-lo", "actor": "examiner" },
            { "id": "to-opp-hi", "from": "start", "to": "opp-hi", "label": "to-opp-hi", "actor": "examiner" }
        ]
    })))
}

#[test]
fn cvar_opponent_is_adversarial() {
    let g = opponent_fork_graph();
    let sc = Scenario {
        objective: Objective::Cvar {
            alpha: 0.5,
            grid: 21,
            y_lo: None,
            y_hi: None,
        },
        ..Default::default()
    };
    let v = View::new(&g, &sc).unwrap();
    let cv = cvar::solve(
        &v,
        0.5,
        21,
        None,
        &litgraph::algo::mdp::SolveOptions::default(),
    )
    .unwrap();
    assert_eq!(
        cv.solution.choice[&v.start],
        g.edge("cvopp::to-opp-lo").unwrap(),
        "an adversarial opponent picks the edge worse for us"
    );
}

#[test]
fn cvar_handles_forced_choice_act_or_wait_ties_and_sinks() {
    let g = coverage_graph();
    let mut policy = std::collections::BTreeMap::new();
    // Forced (worse-looking, by raw payoff) into `mid`, so the reconstructed
    // rollout also crosses the act-or-wait node.
    policy.insert("cvcov::start".to_string(), "cvcov::to-mid".to_string());
    let sc = Scenario {
        objective: Objective::Cvar {
            alpha: 0.3,
            grid: 51,
            y_lo: None,
            y_hi: None,
        },
        mixed: MixedMode::ActOrWait,
        policy,
        ..Default::default()
    };
    let v = View::new(&g, &sc).unwrap();
    let cv = cvar::solve(
        &v,
        0.3,
        51,
        None,
        &litgraph::algo::mdp::SolveOptions::default(),
    )
    .unwrap();
    // The forced choice at `start` (into `mid`, not the directly-terminal
    // options) is respected.
    assert_eq!(
        cv.solution.choice[&v.start],
        g.edge("cvcov::to-mid").unwrap()
    );
    // At `mid`, waiting (`wait-term`, 300) beats acting (`act-term`, -500).
    let mid = g.node("cvcov::mid").unwrap();
    assert_eq!(cv.solution.choice[&mid], WAIT);
    // `cyc-a`/`cyc-b` form a genuine cycle (a chance node spinning between
    // them before exiting), exercising the iterate-to-convergence branch.
    assert!(cv.solution.converged, "{:?}", cv.solution.unconverged);
    // `tie-node` and `dead` are unreachable from `start`, but every node's
    // backward-induction backup still runs (a tied Me choice, and a sink);
    // the reachable part of the solve stayed finite throughout.
    assert!(cv.cvar.is_finite());
}

#[test]
fn cvar_y_range_of_requires_both_bounds_together() {
    assert!(cvar::y_range_of(Some(1.0), None).is_err());
    assert!(cvar::y_range_of(None, Some(1.0)).is_err());
    assert_eq!(cvar::y_range_of(None, None).unwrap(), None);
    assert_eq!(
        cvar::y_range_of(Some(1.0), Some(2.0)).unwrap(),
        Some((1.0, 2.0))
    );
}

#[test]
fn cvar_solve_rejects_a_backwards_y_range() {
    let g = risky_vs_safe_graph();
    let v = View::new(&g, &Scenario::default()).unwrap();
    let opts = litgraph::algo::mdp::SolveOptions::default();
    assert!(cvar::solve(&v, 0.1, 21, Some((5.0, 5.0)), &opts).is_err());
    assert!(cvar::solve(&v, 0.1, 21, Some((10.0, 5.0)), &opts).is_err());
}

#[test]
fn cvar_y_range_override_is_used_verbatim_instead_of_the_default() {
    let g = risky_vs_safe_graph();
    let v = View::new(&g, &Scenario::default()).unwrap();
    let opts = litgraph::algo::mdp::SolveOptions::default();
    // The default range would cover the graph's [-1000, 150] terminal
    // payoffs; this override is deliberately narrower and offset from it.
    let (lo, hi) = (-40.0, 60.0);
    let cv = cvar::solve(&v, 0.1, 21, Some((lo, hi)), &opts).unwrap();
    assert_eq!(cv.y_range, (lo, hi));
}

/// The full request path: `Objective::Cvar`'s `y_lo`/`y_hi` deserialize from
/// JSON and flow through to the `solve` op's reported `y_range`.
#[test]
fn cvar_y_range_override_flows_through_the_solve_op() {
    use litgraph::api::{handle, Catalog, Request};
    let catalog = Catalog::embedded().unwrap();
    let pack = catalog.packs.first().unwrap().1.id.clone();
    let req = Request {
        packs: vec![pack],
        scenario: serde_json::from_value(json!({
            "objective": {"type": "cvar", "alpha": 0.2, "grid": 11, "y_lo": -500.0, "y_hi": 500.0}
        }))
        .unwrap(),
        op: serde_json::from_value(json!({"op": "solve"})).unwrap(),
        ..Default::default()
    };
    let resp = handle(&req, &catalog);
    assert!(resp.ok, "{resp:?}");
    let result = resp.result.unwrap();
    assert_eq!(result["y_range"], json!([-500.0, 500.0]));
}

#[test]
fn cvar_y_lo_without_y_hi_is_a_clear_error_not_a_parse_failure() {
    use litgraph::api::{handle, Catalog, Request};
    let catalog = Catalog::embedded().unwrap();
    let pack = catalog.packs.first().unwrap().1.id.clone();
    let req = Request {
        packs: vec![pack],
        scenario: serde_json::from_value(json!({
            "objective": {"type": "cvar", "alpha": 0.2, "y_lo": -500.0}
        }))
        .unwrap(),
        op: serde_json::from_value(json!({"op": "solve"})).unwrap(),
        ..Default::default()
    };
    let resp = handle(&req, &catalog);
    assert!(!resp.ok);
    assert!(resp.error.unwrap().message.contains("y_lo and y_hi"));
}
