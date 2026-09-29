// SPDX-License-Identifier: GPL-3.0-or-later
//! General-sum opponents (`algo::equilibrium`): zero-sum falls out as the
//! exact special case, a general-sum opponent picks their own best edge
//! (not the adversarial one), both players' values are reported, and cyclic
//! non-convergence is reported honestly rather than silently.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use litgraph::algo::{equilibrium, mdp};
use litgraph::model::{CompileOptions, Graph, LinkFile, Pack};
use litgraph::scenario::{OpponentMode, Scenario, View};
use serde_json::json;

fn pack(j: serde_json::Value) -> Pack {
    serde_json::from_value(j).expect("test pack literal is well-formed")
}

fn compile(p: Pack) -> Graph {
    Graph::compile(&[p], &LinkFile::default(), &CompileOptions::default()).expect("compiles")
}

/// An opponent-controlled fork: `settle` is worse for us (self payoff 50)
/// but better for the opponent under a *general-sum* reading (`opp_payoff`
/// 10 vs 30); `litigate` is better for us (100) and, in this construction,
/// also better for the opponent (30 > 10). An adversarial (zero-sum)
/// opponent minimizes *our* value and picks `settle`; a general-sum
/// opponent maximizing their own payoff picks `litigate`.
fn settle_or_litigate_graph() -> Graph {
    compile(pack(json!({
        "schemaVersion": 2, "id": "gs", "title": "gs", "startNodeId": "start",
        "roles": { "applicant": "self", "examiner": "opponent" },
        "nodes": [
            { "id": "start", "kind": "state", "label": "start" },
            { "id": "settled", "kind": "terminal", "label": "settled", "payoff": 50.0,
              "attrs": { "opp_payoff": 10.0 } },
            { "id": "litigated", "kind": "terminal", "label": "litigated", "payoff": 100.0,
              "attrs": { "opp_payoff": 30.0 } }
        ],
        "edges": [
            { "id": "settle", "from": "start", "to": "settled", "label": "settle", "actor": "examiner" },
            { "id": "litigate", "from": "start", "to": "litigated", "label": "litigate", "actor": "examiner" }
        ]
    })))
}

#[test]
fn zero_sum_opponent_objective_none_reproduces_mdp_solve_exactly() {
    let g = settle_or_litigate_graph();
    let sc = Scenario::default(); // opponent_objective: None, opponent: Auto (both unauthored -> adversarial)
    let v = View::new(&g, &sc).unwrap();
    let direct = mdp::solve(&v, &mdp::SolveOptions::default()).unwrap();
    let eq = equilibrium::resolve(&v, &mdp::SolveOptions::default()).unwrap();
    assert!(!eq.general_sum);
    assert_eq!(direct.value, eq.solution.value);
    assert_eq!(direct.choice, eq.solution.choice);
    assert_eq!(direct.converged, eq.solution.converged);
    // The zero-sum identity: the opponent's value is the negation of ours.
    for n in 0..g.nodes.len() {
        assert!((eq.opponent_value[n] + eq.solution.value[n]).abs() < 1e-12);
    }
    // An adversary minimizes OUR value: settle (50) beats litigate (100) for them.
    assert_eq!(eq.solution.choice[&v.start], g.edge("gs::settle").unwrap());
    assert!((eq.solution.value[v.start] - 50.0).abs() < 1e-9);
}

#[test]
fn general_sum_opponent_maximizes_their_own_payoff_not_ours() {
    let g = settle_or_litigate_graph();
    let sc = Scenario {
        opponent_objective: Some("node.opp_payoff".to_string()),
        ..Default::default()
    };
    let v = View::new(&g, &sc).unwrap();
    let eq = equilibrium::resolve(&v, &mdp::SolveOptions::default()).unwrap();
    assert!(eq.general_sum);
    assert_eq!(
        eq.solution.choice[&v.start],
        g.edge("gs::litigate").unwrap(),
        "the opponent should pick the edge best for THEM (30 > 10), not the one worst for us"
    );
    assert!((eq.solution.value[v.start] - 100.0).abs() < 1e-9);
    assert!((eq.opponent_value[v.start] - 30.0).abs() < 1e-9);
    assert!(eq.solution.converged);
}

#[test]
fn opponent_objective_forces_a_chooser_even_under_opponent_chance_mode() {
    let g = settle_or_litigate_graph();
    // Both opponent edges are unauthored, so plain `Auto`/`Chance` would
    // treat this node as a draw; `opponent_objective` overrides that.
    let sc = Scenario {
        opponent: OpponentMode::Chance,
        opponent_objective: Some("node.opp_payoff".to_string()),
        ..Default::default()
    };
    let v = View::new(&g, &sc).unwrap();
    assert_eq!(
        v.plan[v.start].control,
        litgraph::scenario::Control::Opponent
    );
    let eq = equilibrium::resolve(&v, &mdp::SolveOptions::default()).unwrap();
    assert_eq!(
        eq.solution.choice[&v.start],
        g.edge("gs::litigate").unwrap()
    );
}

/// A two-node general-sum cycle (each side can force the world to spin
/// instead of ending the case): with a generous iteration cap it converges;
/// with a cap of `1` it must not silently report convergence.
fn general_sum_cycle_graph() -> Graph {
    compile(pack(json!({
        "schemaVersion": 2, "id": "gsc", "title": "gsc", "startNodeId": "a",
        "roles": { "applicant": "self", "examiner": "opponent" },
        "nodes": [
            { "id": "a", "kind": "state", "label": "a" },
            { "id": "b", "kind": "state", "label": "b" },
            { "id": "end", "kind": "terminal", "label": "end", "payoff": 10.0, "attrs": { "opp_payoff": 5.0 } }
        ],
        "edges": [
            { "id": "a-loop", "from": "a", "to": "b", "label": "delay", "actor": "examiner", "cost": 1.0 },
            { "id": "a-end", "from": "a", "to": "end", "label": "resolve", "actor": "examiner" },
            { "id": "b-loop", "from": "b", "to": "a", "label": "delay-back", "actor": "examiner", "cost": 1.0 },
            { "id": "b-end", "from": "b", "to": "end", "label": "resolve", "actor": "examiner" }
        ]
    })))
}

#[test]
fn general_sum_cyclic_component_converges_with_enough_iterations() {
    let g = general_sum_cycle_graph();
    let sc = Scenario {
        opponent_objective: Some("node.opp_payoff".to_string()),
        ..Default::default()
    };
    let v = View::new(&g, &sc).unwrap();
    let eq = equilibrium::resolve(&v, &mdp::SolveOptions::default()).unwrap();
    assert!(eq.solution.converged, "{:?}", eq.solution.unconverged);
}

#[test]
fn opponent_objective_with_fee_shift_warns_it_is_ignored() {
    let g = settle_or_litigate_graph();
    let sc = Scenario {
        opponent_objective: Some("node.opp_payoff".to_string()),
        fee_shift: Some(litgraph::scenario::FeeShift {
            fraction: 0.5,
            eligible: None,
        }),
        ..Default::default()
    };
    let v = View::new(&g, &sc).unwrap();
    assert!(v
        .warnings
        .iter()
        .any(|w| w.code == "opponent-objective-ignores-fee-shift"));
}

#[test]
fn general_sum_cyclic_component_reports_non_convergence_honestly() {
    let g = general_sum_cycle_graph();
    let sc = Scenario {
        opponent_objective: Some("node.opp_payoff".to_string()),
        ..Default::default()
    };
    let v = View::new(&g, &sc).unwrap();
    let opts = mdp::SolveOptions {
        epsilon: 1e-12,
        max_iterations: 1,
    };
    let eq = equilibrium::resolve(&v, &opts).unwrap();
    assert!(!eq.solution.converged);
    assert!(!eq.solution.unconverged.is_empty());
}

/// A pack exercising the `backup`/`best_option` branches the graphs above
/// don't reach: a forced (`scenario.policy`) choice, an act-or-wait node
/// where waiting is optimal, a tie between two equal-value choices, and an
/// isolated sink — all processed regardless of reachability from `start`,
/// since the joint backward induction runs over every SCC in the graph.
fn coverage_graph() -> Graph {
    compile(pack(json!({
        "schemaVersion": 2, "id": "gscov", "title": "gscov", "startNodeId": "start",
        "nodes": [
            { "id": "start", "kind": "state", "label": "start" },
            { "id": "mid", "kind": "state", "label": "mid" },
            { "id": "term-b", "kind": "terminal", "label": "term-b", "payoff": -100.0 },
            { "id": "act-term", "kind": "terminal", "label": "act-term", "payoff": -500.0 },
            { "id": "wait-term", "kind": "terminal", "label": "wait-term", "payoff": 300.0 },
            { "id": "tie-node", "kind": "state", "label": "tie-node" },
            { "id": "tie-a", "kind": "terminal", "label": "tie-a", "payoff": 50.0 },
            { "id": "tie-b", "kind": "terminal", "label": "tie-b", "payoff": 50.0 },
            { "id": "dead", "kind": "state", "label": "dead" }
        ],
        "edges": [
            { "id": "to-mid", "from": "start", "to": "mid", "label": "to-mid", "actor": "applicant" },
            { "id": "to-b", "from": "start", "to": "term-b", "label": "to-b", "actor": "applicant" },
            { "id": "act", "from": "mid", "to": "act-term", "label": "act", "actor": "applicant" },
            { "id": "world", "from": "mid", "to": "wait-term", "label": "world", "actor": "either" },
            { "id": "to-tie-a", "from": "tie-node", "to": "tie-a", "label": "to-tie-a", "actor": "applicant" },
            { "id": "to-tie-b", "from": "tie-node", "to": "tie-b", "label": "to-tie-b", "actor": "applicant" }
        ]
    })))
}

#[test]
fn general_sum_handles_forced_choice_act_or_wait_ties_and_sinks() {
    let g = coverage_graph();
    let mut policy = std::collections::BTreeMap::new();
    // Forced into `mid` rather than the directly-terminal `to-b` option.
    policy.insert("gscov::start".to_string(), "gscov::to-mid".to_string());
    let sc = Scenario {
        opponent_objective: Some("payoff".to_string()),
        mixed: litgraph::scenario::MixedMode::ActOrWait,
        policy,
        ..Default::default()
    };
    let v = View::new(&g, &sc).unwrap();
    let eq = equilibrium::resolve(&v, &mdp::SolveOptions::default()).unwrap();
    assert!(eq.general_sum);
    assert_eq!(
        eq.solution.choice[&v.start],
        g.edge("gscov::to-mid").unwrap()
    );
    let mid = g.node("gscov::mid").unwrap();
    assert_eq!(
        eq.solution.choice[&mid],
        litgraph::scenario::WAIT,
        "waiting (300) beats acting (-500) at mid"
    );
    let tie_node = g.node("gscov::tie-node").unwrap();
    assert!(eq.solution.choice.contains_key(&tie_node));
    let dead = g.node("gscov::dead").unwrap();
    assert!((eq.solution.value[dead]).abs() < 1e-12);
    assert!((eq.opponent_value[dead]).abs() < 1e-12);
}

/// `start` is itself an act-or-wait node, forced (`scenario.policy`) to the
/// *world* edge's own id — read as "wait", not "take that edge as ours" (a
/// distinct branch from forcing an ordinary edge; mirrors `tests/cvar.rs`'s
/// `cvar_forced_to_the_world_edge_id_means_wait`).
#[test]
fn general_sum_forced_to_the_world_edge_id_means_wait() {
    let g = compile(pack(json!({
        "schemaVersion": 2, "id": "gsforce", "title": "gsforce", "startNodeId": "start",
        "nodes": [
            { "id": "start", "kind": "state", "label": "start" },
            { "id": "act-term", "kind": "terminal", "label": "act-term", "payoff": 10.0 },
            { "id": "wait-term", "kind": "terminal", "label": "wait-term", "payoff": 20.0 }
        ],
        "edges": [
            { "id": "act", "from": "start", "to": "act-term", "label": "act", "actor": "applicant" },
            { "id": "world", "from": "start", "to": "wait-term", "label": "world", "actor": "either" }
        ]
    })));
    let mut policy = std::collections::BTreeMap::new();
    policy.insert("gsforce::start".to_string(), "gsforce::world".to_string());
    let sc = Scenario {
        opponent_objective: Some("payoff".to_string()),
        mixed: litgraph::scenario::MixedMode::ActOrWait,
        policy,
        ..Default::default()
    };
    let v = View::new(&g, &sc).unwrap();
    let eq = equilibrium::resolve(&v, &mdp::SolveOptions::default()).unwrap();
    assert_eq!(eq.solution.choice[&v.start], litgraph::scenario::WAIT);
}

/// An edge removed by the scenario (`remove_edges`) is inactive: its `q` is
/// `NaN`, not a stale/garbage number.
#[test]
fn general_sum_inactive_edges_report_nan_q() {
    let g = settle_or_litigate_graph();
    let sc = Scenario {
        opponent_objective: Some("node.opp_payoff".to_string()),
        remove_edges: vec!["gs::litigate".to_string()],
        ..Default::default()
    };
    let v = View::new(&g, &sc).unwrap();
    let eq = equilibrium::resolve(&v, &mdp::SolveOptions::default()).unwrap();
    let litigate = g.edge("gs::litigate").unwrap();
    assert!(eq.solution.q[litigate].is_nan());
    // Only `settle` remains: the opponent has no real choice left.
    assert_eq!(eq.solution.choice[&v.start], g.edge("gs::settle").unwrap());
}
