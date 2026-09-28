// SPDX-License-Identifier: GPL-3.0-or-later
//! Coverage for API-layer branches `tests/api_coverage.rs` and
//! `tests/coverage_gaps.rs` don't reach: every `who_decides` control kind,
//! the opponent-specific `best_line` kinds, act-or-wait regret, edge
//! duration/tags rendering, `targets()`'s `tag:`/plain-node/error paths, an
//! empty catalog, and the full embedded catalog's compile notes and
//! warning-location cap. All assertions are on hand-traceable values from a
//! tiny custom pack (not the shipped forums), so the expected shape is
//! obvious from the fixture below.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use litgraph::api::{handle, Catalog, Op, Request};
use litgraph::model::Pack;
use serde_json::json;

/// One pack exercising every control kind (`Me`, `Opponent`, `Chance`,
/// `Sink`, `Terminal`) plus an act-or-wait opponent node, a duration+tags
/// edge, and a tagged terminal for `targets("tag:...")`.
fn gaps_pack() -> Pack {
    serde_json::from_value(json!({
        "schemaVersion": 2, "id": "gaps", "title": "gaps", "startNodeId": "start",
        "roles": { "applicant": "self", "examiner": "opponent", "office": "nature" },
        "nodes": [
            {"id": "start", "kind": "decision", "label": "Start"},
            {"id": "settled", "kind": "terminal", "label": "Settled", "payoff": 100},
            {"id": "chanceNode", "kind": "state", "label": "Chance"},
            {"id": "chanceA", "kind": "terminal", "label": "ChanceA", "payoff": 10},
            {"id": "chanceB", "kind": "terminal", "label": "ChanceB", "payoff": -10},
            {"id": "deadEnd", "kind": "state", "label": "Dead end"},
            {"id": "taggedTerm", "kind": "terminal", "label": "Special", "payoff": 5, "outcome": ["special"]},
            {"id": "startA", "kind": "decision", "label": "A"},
            {"id": "win", "kind": "terminal", "label": "win", "payoff": 100},
            {"id": "lose", "kind": "terminal", "label": "lose", "payoff": -100},
            {"id": "startB", "kind": "decision", "label": "B"},
            {"id": "oppOut", "kind": "terminal", "label": "oppOut", "payoff": -50},
            {"id": "worldOut", "kind": "terminal", "label": "worldOut", "payoff": -1000}
        ],
        "edges": [
            {"id": "settle", "from": "start", "to": "settled", "label": "Settle", "actor": "applicant",
             "duration": {"min": 10, "mode": 20, "max": 30}, "tags": ["settlement", "xyz"]},
            {"id": "advance", "from": "start", "to": "chanceNode", "label": "Advance", "actor": "applicant"},
            {"id": "toSpecial", "from": "start", "to": "taggedTerm", "label": "ToSpecial", "actor": "applicant"},
            {"id": "toA", "from": "chanceNode", "to": "chanceA", "label": "toA", "actor": "office", "probability": 0.5},
            {"id": "toB", "from": "chanceNode", "to": "chanceB", "label": "toB", "actor": "office", "probability": 0.5},
            {"id": "toWin", "from": "startA", "to": "win", "label": "toWin", "actor": "examiner"},
            {"id": "toLose", "from": "startA", "to": "lose", "label": "toLose", "actor": "examiner"},
            {"id": "oppEdge", "from": "startB", "to": "oppOut", "label": "oppEdge", "actor": "examiner"},
            {"id": "worldEdge", "from": "startB", "to": "worldOut", "label": "worldEdge", "actor": "office"}
        ]
    }))
    .expect("gaps pack parses")
}

fn catalog() -> Catalog {
    let p = gaps_pack();
    Catalog::from_files(
        "t".into(),
        vec![("t.json".into(), serde_json::to_string(&p).unwrap())],
    )
    .expect("gaps catalog compiles")
}

fn req(op: Op) -> Request {
    Request {
        packs: vec!["gaps".into()],
        op,
        ..Request::default()
    }
}

fn explain(node: &str) -> serde_json::Value {
    let resp = handle(
        &req(Op::Explain {
            node: Some(node.into()),
            from: None,
        }),
        &catalog(),
    );
    assert!(resp.ok, "{resp:?}");
    resp.result.unwrap()
}

/// `who_decides` for the two controls the shipped-pack tests never reach:
/// a pure-nature node (`Chance`) and a non-terminal with no active out-edges
/// (`Sink`, from a mask-free authoring gap — see the `sink` warning).
#[test]
fn who_decides_covers_chance_and_sink() {
    let chance = explain("chanceNode");
    assert_eq!(chance["control"], json!("chance"));
    assert_eq!(chance["who_decides"], json!("tribunal/chance"));

    let sink = explain("deadEnd");
    assert_eq!(sink["control"], json!("sink"));
    assert_eq!(sink["who_decides"], json!("nobody (dead end)"));
}

/// An adversarial opponent (missing probabilities on the opponent's own
/// edges → `OpponentMode::Auto` falls back to adversarial) is labelled
/// distinctly from a self-controlled node.
#[test]
fn who_decides_marks_the_opponent_as_adversarial() {
    let a = explain("startA");
    assert_eq!(a["control"], json!("opponent"));
    assert_eq!(a["who_decides"], json!("opponent (adversarial)"));
    // Hand computation: the opponent minimizes our value, so it takes the
    // -100 edge over the +100 one.
    assert_eq!(a["value"], json!(-100.0));
}

/// `best_line`'s opponent-specific kinds: a direct opponent choice, and an
/// opponent choosing to let a nature edge fire (`act-or-wait`) because that
/// world edge (-1000) is worse for us than the opponent's own edge (-50).
#[test]
fn best_line_marks_opponent_choice_and_opponent_wait() {
    let c = catalog();
    let a = handle(
        &req(Op::Solve {
            from: Some("startA".into()),
            full_policy: false,
            all_values: false,
            max_steps: 5,
        }),
        &c,
    );
    assert!(a.ok, "{a:?}");
    let ra = a.result.unwrap();
    assert_eq!(ra["best_line"][0]["kind"], json!("opponent-choice"));
    assert_eq!(ra["best_line"][0]["id"], json!("gaps::toLose"));

    let b = handle(
        &req(Op::Solve {
            from: Some("startB".into()),
            full_policy: false,
            all_values: false,
            max_steps: 5,
        }),
        &c,
    );
    assert!(b.ok, "{b:?}");
    let rb = b.result.unwrap();
    // Hand computation: waiting nets -1000 (the world edge) vs -50 for
    // acting; the adversarial opponent picks the worse-for-us option, WAIT.
    assert_eq!(rb["value"], json!(-1000.0));
    assert_eq!(rb["best_line"][0]["kind"], json!("opponent-waits-likely-draw"));
    assert_eq!(rb["best_line"][0]["id"], json!("gaps::worldEdge"));
}

/// At the same act-or-wait opponent node, `explain`'s WAIT pseudo-option
/// carries a regret computed against the actual best option (itself, since
/// WAIT was chosen) — the acted-edge option instead regrets against WAIT.
#[test]
fn wait_option_regret_is_computed_against_the_chosen_option() {
    let r = explain("startB");
    let options = r["options"].as_array().unwrap();
    let wait = options
        .iter()
        .find(|o| o["id"] == json!("WAIT"))
        .expect("a WAIT option");
    assert_eq!(wait["chosen"], json!(true));
    assert_eq!(wait["regret"], json!(0.0));

    let opp_edge = options
        .iter()
        .find(|o| o["id"] == json!("gaps::oppEdge"))
        .expect("the opponent's own edge as an option");
    assert_eq!(opp_edge["chosen"], json!(false));
    // Regret vs. the chosen WAIT: q(oppEdge) - q(WAIT) = -50 - (-1000) = 950.
    assert_eq!(opp_edge["regret"], json!(950.0));
}

/// An edge with `duration` and `tags` renders both in its option JSON.
#[test]
fn option_json_renders_duration_and_tags() {
    let r = explain("start");
    let options = r["options"].as_array().unwrap();
    let settle = options
        .iter()
        .find(|o| o["id"] == json!("gaps::settle"))
        .expect("the settle option");
    assert_eq!(
        settle["elapsed_days"],
        json!({ "min": 10.0, "mode": 20.0, "max": 30.0 })
    );
    assert_eq!(settle["tags"], json!(["settlement", "xyz"]));
}

/// `targets()`'s three branches: `tag:<x>` resolving to a tagged terminal,
/// `tag:<x>` finding none (`NotFound`), and the plain-node fallback (a
/// `to` that is neither `terminals` nor `tag:`-prefixed).
#[test]
fn path_targets_tag_prefix_plain_node_and_missing_tag() {
    let c = catalog();

    let tagged = handle(
        &req(Op::Path {
            from: None,
            to: "tag:special".into(),
            metric: "dollars".into(),
            k: 1,
            report: vec![],
        }),
        &c,
    );
    assert!(tagged.ok, "{tagged:?}");
    let r = tagged.result.unwrap();
    assert_eq!(r["paths"][0]["end"]["id"], json!("gaps::taggedTerm"));
    assert_eq!(r["paths"][0]["edges"], json!(["gaps::toSpecial"]));

    let missing = handle(
        &req(Op::Path {
            from: None,
            to: "tag:missing".into(),
            metric: "dollars".into(),
            k: 1,
            report: vec![],
        }),
        &c,
    );
    assert!(!missing.ok);
    assert_eq!(missing.error.unwrap().code, "not-found");

    let plain = handle(
        &req(Op::Path {
            from: None,
            to: "chanceNode".into(),
            metric: "dollars".into(),
            k: 1,
            report: vec![],
        }),
        &c,
    );
    assert!(plain.ok, "{plain:?}");
    let r = plain.result.unwrap();
    assert_eq!(r["paths"][0]["end"]["id"], json!("gaps::chanceNode"));
}

/// A catalog with zero packs propagates `Graph::compile`'s "no packs given"
/// through `handle` as an `ok: false` response (`api/mod.rs`'s `run()`
/// error path from `Graph::compile(..)?`).
#[test]
fn empty_catalog_reports_no_packs_given() {
    let empty = Catalog::from_files("empty".into(), vec![]).expect("empty catalog");
    let resp = handle(
        &Request {
            packs: vec![],
            op: Op::Graph { node: None },
            ..Request::default()
        },
        &empty,
    );
    assert!(!resp.ok);
    let e = resp.error.unwrap();
    assert_eq!(e.code, "invalid");
    assert_eq!(e.message, "invalid: no packs given");
}

/// The full embedded catalog with links applied produces `Graph::compile`
/// notes (links superseding pack edges, v1 terminals kept absorbing) which
/// `run()` surfaces as a `"compile"`-coded warning group, and produces more
/// than eight `probability-fill`/`mixed-node`-coded locations somewhere,
/// which `group_warnings` caps at eight.
#[test]
fn full_catalog_surfaces_compile_notes_and_caps_warning_locations() {
    let full = Catalog::embedded().expect("embedded packs always compile");
    let resp = handle(
        &Request {
            packs: vec![],
            op: Op::Graph { node: None },
            ..Request::default()
        },
        &full,
    );
    assert!(resp.ok, "{resp:?}");
    let compile = resp
        .warnings
        .iter()
        .find(|w| w.code == "compile")
        .expect("a compile-note warning group");
    assert!(compile.count > 0);
    let capped = resp
        .warnings
        .iter()
        .find(|w| w.count > 8)
        .expect("some warning code fires more than eight times across the full graph");
    assert_eq!(capped.at.len(), 8, "locations must be capped at eight");
}

/// A node whose only move is a costly self-loop makes the MDP solve hit its
/// iteration cap (never converging) and makes the chain solve singular
/// (never absorbing). `handle` surfaces both: a `not-converged` warning on
/// `solve`, and a `numeric`-coded error with a specific hint on `chain`.
fn loop_pack() -> Pack {
    serde_json::from_value(json!({
        "schemaVersion": 2, "id": "loop", "title": "loop", "startNodeId": "loopy",
        "roles": { "applicant": "self" },
        "nodes": [
            {"id": "loopy", "kind": "decision", "label": "Loopy"}
        ],
        "edges": [
            {"id": "spin", "from": "loopy", "to": "loopy", "label": "spin", "actor": "applicant", "hours": 1}
        ]
    }))
    .expect("loop pack parses")
}

#[test]
fn forced_loop_never_converges_and_chain_reports_a_numeric_hint() {
    let p = loop_pack();
    let catalog = Catalog::from_files(
        "t".into(),
        vec![("t.json".into(), serde_json::to_string(&p).unwrap())],
    )
    .unwrap();
    let loop_req = |op| Request {
        packs: vec!["loop".into()],
        op,
        ..Request::default()
    };

    let solved = handle(
        &loop_req(Op::Solve {
            from: None,
            full_policy: false,
            all_values: false,
            max_steps: 5,
        }),
        &catalog,
    );
    assert!(solved.ok, "{solved:?}");
    assert_eq!(solved.result.unwrap()["converged"], json!(false));
    let not_converged = solved
        .warnings
        .iter()
        .find(|w| w.code == "not-converged")
        .expect("a not-converged warning");
    assert_eq!(not_converged.at, vec!["loop::loopy"]);

    let chained = handle(
        &loop_req(Op::Chain {
            from: None,
            metrics: vec!["dollars".into()],
            top: 5,
        }),
        &catalog,
    );
    assert!(!chained.ok);
    let e = chained.error.unwrap();
    assert_eq!(e.code, "numeric");
    assert_eq!(
        e.hint,
        "a forced or optimal choice loops forever; add a mask or policy to break the cycle"
    );

    // `explain`'s per-option "then" (a nested chain from the edge's target)
    // is omitted when that chain errors — here every option's target loops.
    let explained = handle(&loop_req(Op::Explain { node: None, from: None }), &catalog);
    assert!(explained.ok, "{explained:?}");
    let r = explained.result.unwrap();
    assert!(
        r["options"][0].get("then").is_none(),
        "a looping target's chain should not produce a `then` field: {r:?}"
    );
}
