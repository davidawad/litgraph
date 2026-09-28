// SPDX-License-Identifier: GPL-3.0-or-later
//! Targeted tests for branches in `resolve::edge_at`, `lint::lint`,
//! `metrics::{Edge,Terminal}Env`, `scenario::plan` and `scenario::view` that
//! the other suites don't happen to exercise: the direct-id shortcut before
//! `edge_at`'s label fallback, the defensive `bad-pack-start` diagnostic, the
//! deadline/extendable edge variables and the terminal-env function
//! fallback, the adversarial-opponent and self-only mixed-node branches, the
//! "world edges already take all the mass" act-or-wait fallback, and the
//! `View::new` error/warning paths for out-of-range probability overrides,
//! rescaled siblings, a failing `probability_fn`, `scenario.policy`, a
//! negative cost metric, and a failing utility expression.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use litgraph::lint::lint;
use litgraph::model::{CompileOptions, Graph, LinkFile, Pack};
use litgraph::scenario::{Control, MixedMode, OpponentMode, Scenario, View};
use serde_json::json;
use std::collections::BTreeMap;

fn pack(v: serde_json::Value) -> Pack {
    serde_json::from_value(v).expect("test pack literal is well-formed")
}

fn compile(packs: &[Pack]) -> Graph {
    Graph::compile(packs, &LinkFile::default(), &CompileOptions::default()).expect("compiles")
}

fn view<'a>(g: &'a Graph, sc: &Scenario) -> View<'a> {
    View::new(g, sc).expect("scenario resolves")
}

// --- lint::lint: defensive bad-pack-start ----------------------------------

#[test]
fn lint_reports_bad_pack_start_when_a_packs_own_metadata_is_inconsistent() {
    // `lint` never trusts a pack's own recorded start blindly (its own doc
    // comment: "any lookup that still fails is reported as a diagnostic
    // rather than unwrapped"). A normally-compiled graph always has a start
    // that resolves; we simulate the only way this can go wrong (mismatched
    // metadata) by mutating the compiled `PackMeta` directly.
    let p = pack(json!({
        "schemaVersion": 2, "id": "bp", "title": "T", "startNodeId": "s",
        "nodes": [{"id": "s", "label": "S", "kind": "terminal", "payoff": 1.0}],
        "edges": [],
        "sources": [{"id": "src", "url": "https://example.com"}]
    }));
    let mut g = compile(std::slice::from_ref(&p));
    g.packs[0].start = "bp::does-not-exist".into();
    let diags = lint(&g, &[p]);
    let d = diags
        .iter()
        .find(|d| d.code == "bad-pack-start")
        .expect("bad-pack-start diagnostic");
    assert_eq!(d.severity, "error");
    assert_eq!(d.at, "bp");
    assert!(d.message.contains("does not resolve to a node"));
}

// --- resolve::edge_at direct-id shortcut ------------------------------------

#[test]
fn edge_at_prefers_the_direct_id_over_a_label_search() {
    let p = pack(json!({
        "schemaVersion": 2, "id": "ea2", "title": "T", "startNodeId": "s",
        "nodes": [
            {"id": "s", "label": "S"},
            {"id": "t1", "label": "Go", "kind": "terminal", "payoff": 1.0},
            {"id": "t2", "label": "Also Go", "kind": "terminal", "payoff": 2.0}
        ],
        "edges": [
            {"id": "the-edge", "from": "s", "to": "t1", "label": "Go"},
            {"from": "s", "to": "t2", "label": "Also Go"}
        ]
    }));
    let g = compile(&[p]);
    let s = g.node("ea2::s").unwrap();
    // "the-edge" resolves directly by id (belongs to `s`) rather than by
    // scanning out-edges for a label match.
    let e = g.edge_at(s, "the-edge").unwrap();
    assert_eq!(g.edges[e].id, "ea2::the-edge");
}

#[test]
fn edge_at_falls_back_to_label_when_the_resolved_id_belongs_elsewhere() {
    // "aid" resolves to an edge id, but that edge belongs to a DIFFERENT
    // node than the one we're asking about — edge_at must fall through to
    // the label search on THIS node's own out-edges instead of returning it.
    let p = pack(json!({
        "schemaVersion": 2, "id": "ea3", "title": "T", "startNodeId": "s",
        "nodes": [
            {"id": "s", "label": "S"},
            {"id": "u", "label": "U"},
            {"id": "t1", "label": "T1", "kind": "terminal", "payoff": 1.0},
            {"id": "t2", "label": "T2", "kind": "terminal", "payoff": 2.0}
        ],
        "edges": [
            {"id": "aid", "from": "s", "to": "t1", "label": "Go"},
            {"from": "u", "to": "t2", "label": "aid"}
        ]
    }));
    let g = compile(&[p]);
    let u = g.node("ea3::u").unwrap();
    let e = g.edge_at(u, "aid").unwrap();
    assert_eq!(g.edges[e].from, u);
    assert_eq!(g.edges[e].label, "aid");
}

// --- metrics::EdgeEnv: deadline/extendable variables ------------------------

fn deadline_pack() -> Pack {
    pack(json!({
        "schemaVersion": 2, "id": "dl", "title": "T", "startNodeId": "s",
        "nodes": [
            {"id": "s", "label": "S"},
            {"id": "t", "label": "T", "kind": "terminal", "payoff": 1.0}
        ],
        "edges": [{
            "from": "s", "to": "t", "label": "file", "actor": "applicant",
            "deadline": {"length": 14.0, "extendable": true}
        }]
    }))
}

#[test]
fn edge_env_reports_elapsed_bounds_and_extendable_from_the_deadline() {
    let g = compile(&[deadline_pack()]);
    let v = view(&g, &Scenario::default());
    let e = g.out[g.node("dl::s").unwrap()][0];
    // No `duration`, so elapsed/elapsed_min/elapsed_max all fall back to the
    // deadline length; `extendable` reads the deadline's own flag.
    let vals = v
        .metric("elapsed_min + elapsed_max + has_deadline * 100 + extendable * 1000")
        .unwrap();
    assert_eq!(vals[e], 14.0 + 14.0 + 100.0 + 1000.0);
}

#[test]
fn edge_env_extendable_defaults_to_false_with_no_deadline() {
    let p = pack(json!({
        "schemaVersion": 2, "id": "nodl", "title": "T", "startNodeId": "s",
        "nodes": [
            {"id": "s", "label": "S"},
            {"id": "t", "label": "T", "kind": "terminal", "payoff": 1.0}
        ],
        "edges": [{"from": "s", "to": "t", "label": "go"}]
    }));
    let g = compile(&[p]);
    let v = view(&g, &Scenario::default());
    let e = g.out[g.node("nodl::s").unwrap()][0];
    assert_eq!(v.metric("extendable").unwrap()[e], 0.0);
    assert_eq!(v.metric("has_deadline").unwrap()[e], 0.0);
}

// --- metrics::TerminalEnv::func fallback to math() --------------------------

#[test]
fn terminal_env_falls_back_to_a_math_function_it_does_not_know_itself() {
    // TerminalEnv::func only knows tag/to_tag/pack/label_has; any other
    // function name (here a genuine math builtin) returns None and falls
    // through to `math()` in the expression evaluator.
    let p = pack(json!({
        "schemaVersion": 2, "id": "absu", "title": "T", "startNodeId": "s",
        "nodes": [
            {"id": "s", "label": "S"},
            {"id": "t", "label": "T", "kind": "terminal", "payoff": -50.0}
        ],
        "edges": [{"from": "s", "to": "t", "label": "go"}]
    }));
    let g = compile(&[p]);
    let sc = Scenario {
        utility: Some("abs(payoff)".into()),
        ..Scenario::default()
    };
    let v = view(&g, &sc);
    let t = g.node("absu::t").unwrap();
    assert_eq!(v.utility[t], 50.0);
}

// --- scenario::plan: opponent Adversarial / mixed SelfOnly / NatureFirst ---

#[test]
fn adversarial_opponent_is_a_minimizing_chooser() {
    let p = pack(json!({
        "schemaVersion": 2, "id": "adv", "title": "T", "startNodeId": "s",
        "roles": {"rival": "opponent"},
        "nodes": [
            {"id": "s", "label": "S"},
            {"id": "t1", "label": "T1", "kind": "terminal", "payoff": 10.0},
            {"id": "t2", "label": "T2", "kind": "terminal", "payoff": -10.0}
        ],
        "edges": [
            {"from": "s", "to": "t1", "label": "a", "actor": "rival"},
            {"from": "s", "to": "t2", "label": "b", "actor": "rival"}
        ]
    }));
    let g = compile(&[p]);
    let sc = Scenario {
        opponent: OpponentMode::Adversarial,
        ..Scenario::default()
    };
    let v = view(&g, &sc);
    let s = g.node("adv::s").unwrap();
    assert_eq!(v.plan[s].control, Control::Opponent);
    assert!(v.plan[s].minimize);
    assert_eq!(v.plan[s].choices.len(), 2);
}

#[test]
fn self_only_mixed_mode_ignores_world_edges_entirely() {
    let p = pack(json!({
        "schemaVersion": 2, "id": "selfonly", "title": "T", "startNodeId": "s",
        "nodes": [
            {"id": "s", "label": "S"},
            {"id": "mine", "label": "Mine", "kind": "terminal", "payoff": 5.0},
            {"id": "world", "label": "World", "kind": "terminal", "payoff": -5.0}
        ],
        "edges": [
            {"from": "s", "to": "mine", "label": "move", "actor": "applicant"},
            {"from": "s", "to": "world", "label": "wait", "actor": "examiner"}
        ]
    }));
    let g = compile(&[p]);
    let sc = Scenario {
        mixed: MixedMode::SelfOnly,
        ..Scenario::default()
    };
    let v = view(&g, &sc);
    let s = g.node("selfonly::s").unwrap();
    let mine_edge = g.edge_at(s, "move").unwrap();
    assert_eq!(v.plan[s].control, Control::Me);
    assert_eq!(v.plan[s].choices, vec![mine_edge]);
    assert!(v.plan[s].wait.is_empty());
}

#[test]
fn nature_first_falls_back_to_act_or_wait_when_world_edges_take_all_the_mass() {
    // Both world edges are fully authored and already sum to 1.0, so there
    // is no residual mass left for `nature-first` to give the chooser: the
    // node is modeled as act-or-wait instead, with a warning explaining why.
    let p = pack(json!({
        "schemaVersion": 2, "id": "nf", "title": "T", "startNodeId": "s",
        "nodes": [
            {"id": "s", "label": "S"},
            {"id": "mine", "label": "Mine", "kind": "terminal", "payoff": 5.0},
            {"id": "w1", "label": "W1", "kind": "terminal", "payoff": 1.0},
            {"id": "w2", "label": "W2", "kind": "terminal", "payoff": 2.0}
        ],
        "edges": [
            {"from": "s", "to": "mine", "label": "move", "actor": "applicant"},
            {"from": "s", "to": "w1", "label": "w1", "actor": "examiner", "probability": 0.5},
            {"from": "s", "to": "w2", "label": "w2", "actor": "office", "probability": 0.5}
        ]
    }));
    let g = compile(&[p]);
    let v = view(&g, &Scenario::default());
    let s = g.node("nf::s").unwrap();
    assert_eq!(v.plan[s].control, Control::Me);
    assert_eq!(v.plan[s].wait.len(), 2);
    let msg = v
        .warnings
        .iter()
        .find(|w| w.code == "mixed-node")
        .expect("mixed-node warning");
    assert!(msg.message.contains("take all the probability mass"));
}

// --- scenario::view: probability overrides, probability_fn, policy, ------
// --- negative cost, and a failing utility expression -----------------------

fn chance_pack() -> Pack {
    pack(json!({
        "schemaVersion": 2, "id": "prob", "title": "T", "startNodeId": "s",
        "nodes": [
            {"id": "s", "label": "S"},
            {"id": "t1", "label": "T1", "kind": "terminal", "payoff": 1.0},
            {"id": "t2", "label": "T2", "kind": "terminal", "payoff": 2.0}
        ],
        "edges": [
            {"id": "e1", "from": "s", "to": "t1", "label": "e1", "actor": "examiner", "probability": 0.6},
            {"id": "e2", "from": "s", "to": "t2", "label": "e2", "actor": "office", "probability": 0.4}
        ]
    }))
}

#[test]
fn scenario_probability_override_out_of_range_is_an_error() {
    let g = compile(&[chance_pack()]);
    let sc = Scenario {
        probabilities: BTreeMap::from([("prob::e1".to_string(), 1.5)]),
        ..Scenario::default()
    };
    let err = View::new(&g, &sc).err().unwrap();
    assert_eq!(err.code(), "invalid");
    assert!(err.to_string().contains("must be in [0,1]"));
}

#[test]
fn scenario_probability_override_rescales_authored_siblings() {
    let g = compile(&[chance_pack()]);
    let sc = Scenario {
        probabilities: BTreeMap::from([("prob::e1".to_string(), 0.9)]),
        ..Scenario::default()
    };
    let v = view(&g, &sc);
    let e1 = g.edge("prob::e1").unwrap();
    let e2 = g.edge("prob::e2").unwrap();
    assert!((v.prob[e1].unwrap() - 0.9).abs() < 1e-9);
    // e2 (originally 0.4) is rescaled to keep the node's total mass at 1.0.
    assert!((v.prob[e2].unwrap() - 0.1).abs() < 1e-9);
}

#[test]
fn scenario_probability_override_with_no_authored_siblings_skips_rescaling() {
    // When the overridden edge has no OTHER authored sibling at its node,
    // `rest` is 0 and the rescale loop is skipped entirely (only the
    // override itself applies).
    let p = pack(json!({
        "schemaVersion": 2, "id": "single", "title": "T", "startNodeId": "s",
        "nodes": [
            {"id": "s", "label": "S"},
            {"id": "t", "label": "T", "kind": "terminal", "payoff": 1.0}
        ],
        "edges": [{"id": "e1", "from": "s", "to": "t", "label": "e1", "actor": "examiner"}]
    }));
    let g = compile(&[p]);
    let sc = Scenario {
        probabilities: BTreeMap::from([("single::e1".to_string(), 1.0)]),
        ..Scenario::default()
    };
    let v = view(&g, &sc);
    let e1 = g.edge("single::e1").unwrap();
    assert_eq!(v.prob[e1], Some(1.0));
}

#[test]
fn probability_fn_error_is_reported_with_the_offending_edge() {
    let g = compile(&[chance_pack()]);
    let sc = Scenario {
        probability_fn: Some("totally_unknown_var".into()),
        ..Scenario::default()
    };
    let err = View::new(&g, &sc).err().unwrap();
    assert_eq!(err.code(), "expr");
    assert!(err.to_string().contains("probability_fn on"));
}

#[test]
fn scenario_policy_forces_a_nodes_choice() {
    let p = pack(json!({
        "schemaVersion": 2, "id": "pol", "title": "T", "startNodeId": "s",
        "nodes": [
            {"id": "s", "label": "S"},
            {"id": "t1", "label": "T1", "kind": "terminal", "payoff": 1.0},
            {"id": "t2", "label": "T2", "kind": "terminal", "payoff": 2.0}
        ],
        "edges": [
            {"from": "s", "to": "t1", "label": "left", "actor": "applicant"},
            {"from": "s", "to": "t2", "label": "right", "actor": "applicant"}
        ]
    }));
    let g = compile(&[p]);
    let sc = Scenario {
        policy: BTreeMap::from([("pol::s".to_string(), "right".to_string())]),
        ..Scenario::default()
    };
    let v = view(&g, &sc);
    let s = g.node("pol::s").unwrap();
    let right = g.edge_at(s, "right").unwrap();
    assert_eq!(v.forced.get(&s), Some(&right));
}

#[test]
fn a_negative_cost_metric_warns_instead_of_erroring() {
    let p = pack(json!({
        "schemaVersion": 2, "id": "neg", "title": "T", "startNodeId": "s",
        "nodes": [
            {"id": "s", "label": "S"},
            {"id": "t", "label": "T", "kind": "terminal", "payoff": 1.0}
        ],
        "edges": [{"from": "s", "to": "t", "label": "go"}]
    }));
    let g = compile(&[p]);
    let sc = Scenario {
        cost: Some("-1".into()),
        ..Scenario::default()
    };
    let v = view(&g, &sc);
    let w = v
        .warnings
        .iter()
        .find(|w| w.code == "negative-cost")
        .expect("negative-cost warning");
    assert!(w.message.contains("cost metric `-1` is negative"));
}

#[test]
fn a_failing_utility_expression_is_reported_with_the_terminal() {
    let p = pack(json!({
        "schemaVersion": 2, "id": "badu", "title": "T", "startNodeId": "s",
        "nodes": [
            {"id": "s", "label": "S"},
            {"id": "t", "label": "T", "kind": "terminal", "payoff": 1.0}
        ],
        "edges": [{"from": "s", "to": "t", "label": "go"}]
    }));
    let g = compile(&[p]);
    let sc = Scenario {
        utility: Some("nonexistent_var".into()),
        ..Scenario::default()
    };
    let err = View::new(&g, &sc).err().unwrap();
    assert_eq!(err.code(), "expr");
    assert!(err
        .to_string()
        .contains("utility `nonexistent_var` at badu::t"));
}
