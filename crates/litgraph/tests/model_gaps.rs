// SPDX-License-Identifier: GPL-3.0-or-later
//! Targeted tests for `model::{graph, resolve, schema}` branches the other
//! suites don't happen to exercise: compile-time structural errors (empty
//! pack list, duplicate pack/node/edge ids, an unresolved pack start, a link
//! that replaces an edge that doesn't exist), the already-qualified-id
//! shortcut in `qualify`, the mixed-actor fallback when a terminal with
//! out-edges is continued, the direct-id shortcut in `edge_at`, and the two
//! `Pack::from_json` parse errors (unsupported `schemaVersion`, `replaces`
//! authored outside `links.json`).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use litgraph::model::{qualify, CompileOptions, Graph, LinkFile, NodeKind, Pack, RawEdge};
use serde_json::json;

fn pack(v: serde_json::Value) -> Pack {
    serde_json::from_value(v).expect("test pack literal is well-formed")
}

// --- graph::qualify -------------------------------------------------------

#[test]
fn qualify_prefixes_a_bare_local_id_but_leaves_a_qualified_one_alone() {
    assert_eq!(qualify("pack", "node"), "pack::node");
    assert_eq!(qualify("pack", "other::node"), "other::node");
}

// --- Graph::compile structural errors --------------------------------------

#[test]
fn compile_rejects_an_empty_pack_list() {
    let err = Graph::compile(&[], &LinkFile::default(), &CompileOptions::default()).unwrap_err();
    assert_eq!(err.code(), "invalid");
    assert!(err.to_string().contains("no packs given"));
}

#[test]
fn compile_rejects_the_same_pack_id_twice() {
    let p = pack(json!({
        "schemaVersion": 2, "id": "dup", "title": "Dup", "startNodeId": "s",
        "nodes": [{"id": "s", "label": "S", "kind": "terminal", "payoff": 1.0}],
        "edges": []
    }));
    let err = Graph::compile(
        &[p.clone(), p],
        &LinkFile::default(),
        &CompileOptions::default(),
    )
    .unwrap_err();
    assert_eq!(err.code(), "invalid");
    assert!(err.to_string().contains("given twice"));
}

#[test]
fn compile_rejects_a_duplicate_node_id_within_one_pack() {
    let p = pack(json!({
        "schemaVersion": 2, "id": "dupnode", "title": "T", "startNodeId": "x",
        "nodes": [
            {"id": "x", "label": "X"},
            {"id": "x", "label": "X again", "kind": "terminal", "payoff": 1.0}
        ],
        "edges": []
    }));
    let err = Graph::compile(&[p], &LinkFile::default(), &CompileOptions::default()).unwrap_err();
    assert_eq!(err.code(), "invalid");
    assert!(err.to_string().contains("duplicate node id"));
}

#[test]
fn compile_rejects_an_unresolved_start_node_id() {
    let p = pack(json!({
        "schemaVersion": 2, "id": "nostart", "title": "T", "startNodeId": "missing",
        "nodes": [{"id": "only", "label": "Only", "kind": "terminal", "payoff": 1.0}],
        "edges": []
    }));
    let err = Graph::compile(&[p], &LinkFile::default(), &CompileOptions::default()).unwrap_err();
    assert_eq!(err.code(), "invalid");
    assert!(err
        .to_string()
        .contains("startNodeId nostart::missing not found"));
}

#[test]
fn compile_rejects_a_duplicate_edge_id_within_one_pack() {
    let p = pack(json!({
        "schemaVersion": 2, "id": "dupedge", "title": "T", "startNodeId": "s",
        "nodes": [
            {"id": "s", "label": "S"},
            {"id": "t1", "label": "T1", "kind": "terminal", "payoff": 1.0},
            {"id": "t2", "label": "T2", "kind": "terminal", "payoff": 2.0}
        ],
        "edges": [
            {"id": "dup", "from": "s", "to": "t1", "label": "go1"},
            {"id": "dup", "from": "s", "to": "t2", "label": "go2"}
        ]
    }));
    let err = Graph::compile(&[p], &LinkFile::default(), &CompileOptions::default()).unwrap_err();
    assert_eq!(err.code(), "invalid");
    assert!(err.to_string().contains("duplicate edge id"));
}

#[test]
fn compile_rejects_a_link_that_replaces_an_edge_that_does_not_exist() {
    let p1 = pack(json!({
        "schemaVersion": 2, "id": "p1", "title": "P1", "startNodeId": "a",
        "nodes": [{"id": "a", "label": "A", "kind": "terminal", "payoff": 1.0}],
        "edges": []
    }));
    let p2 = pack(json!({
        "schemaVersion": 2, "id": "p2", "title": "P2", "startNodeId": "b",
        "nodes": [{"id": "b", "label": "B", "kind": "terminal", "payoff": 2.0}],
        "edges": []
    }));
    let lf = LinkFile {
        links: vec![RawEdge {
            id: Some("appeal".into()),
            from: "p1::a".into(),
            to: "p2::b".into(),
            label: "appeal".into(),
            actor: "either".into(),
            replaces: vec!["p1::nonexistent".into()],
            ..Default::default()
        }],
        ..Default::default()
    };
    let err = Graph::compile(&[p1, p2], &lf, &CompileOptions::default()).unwrap_err();
    assert_eq!(err.code(), "invalid");
    assert!(err.to_string().contains("replaces unknown edge"));
}

// --- graph::continue_terminal: mixed-actor accept edge ---------------------

#[test]
fn continued_terminal_with_mixed_actors_gets_an_either_accept_edge() {
    // A v2 terminal with out-edges is continued into a choice with an
    // explicit `#accept` edge; when its out-edges don't share one actor the
    // accept edge's actor falls back to "either".
    let p = pack(json!({
        "schemaVersion": 2, "id": "cont", "title": "T", "startNodeId": "s",
        "nodes": [
            {"id": "s", "label": "S"},
            {"id": "term", "label": "Term", "kind": "terminal", "payoff": 10.0, "outcome": ["win"]},
            {"id": "next1", "label": "Next1", "kind": "terminal", "payoff": 20.0},
            {"id": "next2", "label": "Next2", "kind": "terminal", "payoff": -5.0}
        ],
        "edges": [
            {"from": "s", "to": "term", "label": "go", "actor": "applicant"},
            {"from": "term", "to": "next1", "label": "appeal", "actor": "applicant"},
            {"from": "term", "to": "next2", "label": "cross-appeal", "actor": "examiner"}
        ]
    }));
    let g = Graph::compile(&[p], &LinkFile::default(), &CompileOptions::default()).unwrap();
    let term = g.node("cont::term").unwrap();
    assert_eq!(g.nodes[term].kind, NodeKind::Decision);
    let accept = g.edge("cont::term#accept").unwrap();
    assert_eq!(g.edges[accept].actor, "either");
}

// --- resolve::edge_at direct-id shortcut -----------------------------------

#[test]
fn edge_at_resolves_directly_by_id_before_falling_back_to_label() {
    let p = pack(json!({
        "schemaVersion": 2, "id": "ea", "title": "T", "startNodeId": "s",
        "nodes": [
            {"id": "s", "label": "S"},
            {"id": "t", "label": "T", "kind": "terminal", "payoff": 1.0}
        ],
        "edges": [{"id": "go-edge", "from": "s", "to": "t", "label": "Go"}]
    }));
    let g = Graph::compile(&[p], &LinkFile::default(), &CompileOptions::default()).unwrap();
    let s = g.node("ea::s").unwrap();
    let e = g.edge_at(s, "go-edge").unwrap();
    assert_eq!(g.edges[e].from, s);
    assert_eq!(g.edges[e].id, "ea::go-edge");
}

// --- schema::Pack::from_json parse errors ----------------------------------

#[test]
fn pack_from_json_rejects_an_unsupported_schema_version() {
    let err = Pack::from_json(
        r#"{"schemaVersion": 3, "id": "x", "title": "T", "startNodeId": "s",
            "nodes": [{"id": "s", "label": "S", "kind": "terminal", "payoff": 1.0}], "edges": []}"#,
    )
    .unwrap_err();
    assert_eq!(err.code(), "parse");
    assert!(err.to_string().contains("unsupported schemaVersion"));
}

#[test]
fn pack_from_json_rejects_replaces_authored_outside_links_json() {
    let err = Pack::from_json(
        r#"{"schemaVersion": 2, "id": "x", "title": "T", "startNodeId": "s",
            "nodes": [{"id": "s", "label": "S", "kind": "terminal", "payoff": 1.0}],
            "edges": [{"from": "s", "to": "s", "label": "loop", "replaces": ["other::edge"]}]}"#,
    )
    .unwrap_err();
    assert_eq!(err.code(), "parse");
    assert!(err.to_string().contains("only valid in links.json"));
}
