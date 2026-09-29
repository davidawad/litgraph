// SPDX-License-Identifier: GPL-3.0-or-later
//! Unit tests for `view.rs`.

#![allow(clippy::unwrap_used)]
use super::*;
use crate::model::{CompileOptions, LinkFile, Pack};

/// A `fact`-tagged decision with two `office` (nature) out-edges and an
/// authored 90/10 prior, mirroring `cofc::limitations-check`.
fn fact_pack(json: &str) -> Graph {
    let pack = Pack::from_json(json).unwrap();
    Graph::compile(&[pack], &LinkFile::default(), &CompileOptions::default()).unwrap()
}

const FACT_JSON: &str = r#"{
    "schemaVersion": 2, "id": "demo", "title": "Demo", "startNodeId": "check",
    "nodes": [
        {"id": "check", "label": "Check", "kind": "decision", "tags": ["fact"]},
        {"id": "ok", "label": "OK", "kind": "terminal", "payoff": 1000.0},
        {"id": "barred", "label": "Barred", "kind": "terminal", "payoff": 0.0}
    ],
    "edges": [
        {"id": "e-ok", "from": "check", "to": "ok", "label": "clear", "actor": "office", "probability": 0.9},
        {"id": "e-barred", "from": "check", "to": "barred", "label": "barred", "actor": "office", "probability": 0.1}
    ]
}"#;

#[test]
fn unset_fact_uses_the_authored_prior_and_warns() {
    let g = fact_pack(FACT_JSON);
    let v = View::new(&g, &Scenario::default()).unwrap();
    let check = g.node("demo::check").unwrap();
    let ok = g.edge("demo::e-ok").unwrap();
    let barred = g.edge("demo::e-barred").unwrap();
    assert_eq!(v.prob[ok], Some(0.9));
    assert_eq!(v.prob[barred], Some(0.1));
    assert!(v
        .warnings
        .iter()
        .any(|w| w.code == "fact-unset" && w.at.as_deref() == Some(g.nodes[check].id.as_str())));
}

#[test]
fn set_fact_forces_its_edge_and_zeroes_the_sibling_without_warning() {
    let g = fact_pack(FACT_JSON);
    let sc = Scenario {
        facts: BTreeMap::from([("demo::check".into(), "demo::e-barred".into())]),
        ..Scenario::default()
    };
    let v = View::new(&g, &sc).unwrap();
    let ok = g.edge("demo::e-ok").unwrap();
    let barred = g.edge("demo::e-barred").unwrap();
    assert_eq!(v.prob[barred], Some(1.0));
    assert_eq!(v.prob[ok], Some(0.0));
    assert!(!v.warnings.iter().any(|w| w.code == "fact-unset"));
    assert!(!v.warnings.iter().any(|w| w.code == "probability-fill"));
}

#[test]
fn a_non_fact_chance_node_is_unaffected() {
    // Same shape, no `fact` tag: plain chance-node warnings still apply
    // (regression guard: the fact path must not swallow ordinary nodes).
    const JSON: &str = r#"{
        "schemaVersion": 2, "id": "demo", "title": "Demo", "startNodeId": "check",
        "nodes": [
            {"id": "check", "label": "Check", "kind": "decision"},
            {"id": "a", "label": "A", "kind": "terminal", "payoff": 1.0},
            {"id": "b", "label": "B", "kind": "terminal", "payoff": 0.0}
        ],
        "edges": [
            {"from": "check", "to": "a", "label": "a", "actor": "office"},
            {"from": "check", "to": "b", "label": "b", "actor": "office"}
        ]
    }"#;
    let g = fact_pack(JSON);
    let v = View::new(&g, &Scenario::default()).unwrap();
    assert!(v.warnings.iter().any(|w| w.code == "probability-fill"));
    assert!(!v.warnings.iter().any(|w| w.code == "fact-unset"));
}

/// A fact is authored once for the pack node it describes, not once per
/// state-flag history that node can be reached under (same reasoning as
/// the calibration fix, docs/PACK_SCHEMA.md#state-flags): forcing
/// `demo::check`'s fact must also force `demo::check{remanded}`'s copy of
/// the same edge, not just the base node's.
#[test]
fn a_fact_set_on_the_base_node_also_forces_its_flagged_copies() {
    const FLAGGED_FACT_JSON: &str = r#"{
        "schemaVersion": 2, "id": "demo", "title": "Demo", "startNodeId": "check",
        "nodes": [
            {"id": "check", "label": "Check", "kind": "decision", "tags": ["fact"]},
            {"id": "ok", "label": "OK", "kind": "terminal", "payoff": 1000.0},
            {"id": "barred", "label": "Barred", "kind": "terminal", "payoff": 0.0},
            {"id": "remand", "label": "Remand", "kind": "state"}
        ],
        "edges": [
            {"id": "e-ok", "from": "check", "to": "ok", "label": "clear", "actor": "office", "probability": 0.9},
            {"id": "e-barred", "from": "check", "to": "barred", "label": "barred", "actor": "office", "probability": 0.1},
            {"id": "re-check", "from": "remand", "to": "check", "label": "re-check", "actor": "office", "sets": ["remanded"]}
        ]
    }"#;
    let g = fact_pack(FLAGGED_FACT_JSON);
    // The flagged copy must actually exist, or this test would pass for
    // the wrong reason.
    let check_flagged = g.node("demo::check{remanded}").expect(
        "check{remanded} should exist: remand->check sets the flag before re-entering check",
    );
    assert!(g.nodes[check_flagged].has_tag("fact"));

    let sc = Scenario {
        facts: BTreeMap::from([("demo::check".into(), "demo::e-barred".into())]),
        ..Scenario::default()
    };
    let v = View::new(&g, &sc).unwrap();

    let ok = g.edge("demo::e-ok").unwrap();
    let barred = g.edge("demo::e-barred").unwrap();
    assert_eq!(v.prob[barred], Some(1.0));
    assert_eq!(v.prob[ok], Some(0.0));

    let ok_flagged = g.edge("demo::e-ok{remanded}").unwrap();
    let barred_flagged = g.edge("demo::e-barred{remanded}").unwrap();
    assert_eq!(
        v.prob[barred_flagged],
        Some(1.0),
        "the flagged copy's edge must be forced too, not just the base node's"
    );
    assert_eq!(v.prob[ok_flagged], Some(0.0));

    assert!(!v.warnings.iter().any(|w| w.code == "fact-unset"));
}
