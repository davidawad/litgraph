// SPDX-License-Identifier: GPL-3.0-or-later
//! Unit tests for `lint.rs`.

#![allow(clippy::unwrap_used)]
use super::*;
use crate::model::{CompileOptions, LinkFile, RawNode, Source};

fn pack_with_cite(sources: Vec<Source>, cite: &str) -> Pack {
    Pack {
        schema_version: 2,
        id: "test".into(),
        title: "Test".into(),
        description: None,
        jurisdiction: None,
        forum: None,
        start_node_id: "n1".into(),
        groups: vec![],
        roles: std::collections::BTreeMap::new(),
        sources,
        nodes: vec![RawNode {
            id: "n1".into(),
            label: "n1".into(),
            cite: Some(cite.to_string()),
            ..Default::default()
        }],
        edges: vec![],
    }
}

fn source(path: Option<&str>) -> Source {
    Source {
        id: "s".into(),
        title: None,
        url: None,
        path: path.map(str::to_string),
        sha256: None,
        as_of: None,
    }
}

fn frcp_corpus() -> SourceCorpus {
    SourceCorpus::from_files([(
        "sources/frcp-fixture.txt".to_string(),
        "## Rule 12\nDefenses and objections.\n".to_string(),
    )])
}

#[test]
fn reports_unverifiable_cite_with_reason() {
    let p = pack_with_cite(vec![source(Some("sources/frcp-fixture.txt"))], "FRCP 99");
    let mut diags = vec![];
    let mut push = |severity, code, at: String, message: String| {
        diags.push(Diagnostic {
            severity,
            code,
            at,
            message,
        });
    };
    lint_pack_cites(&p, &frcp_corpus(), &mut push);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].code, "unverifiable-cite");
    assert_eq!(diags[0].severity, "warn");
    assert!(diags[0].message.contains("Rule 99"));
}

#[test]
fn no_diagnostic_for_cite_that_resolves() {
    let p = pack_with_cite(vec![source(Some("sources/frcp-fixture.txt"))], "FRCP 12");
    let mut diags = vec![];
    let mut push = |severity, code, at: String, message: String| {
        diags.push(Diagnostic {
            severity,
            code,
            at,
            message,
        });
    };
    lint_pack_cites(&p, &frcp_corpus(), &mut push);
    assert!(diags.is_empty());
}

/// A pack with zero `sources` is already covered by `no-sources`
/// (`lint_pack_sources`); `lint_pack_cites` must not pile on a
/// duplicate per-cite diagnostic.
#[test]
fn no_duplicate_diagnostic_when_pack_has_no_sources() {
    let p = pack_with_cite(vec![], "FRCP 12(b)(6)");
    let mut diags = vec![];
    let mut push = |severity, code, at: String, message: String| {
        diags.push(Diagnostic {
            severity,
            code,
            at,
            message,
        });
    };
    lint_pack_cites(&p, &frcp_corpus(), &mut push);
    assert!(diags.is_empty());
}

#[test]
fn case_citation_produces_no_diagnostic() {
    let p = pack_with_cite(
        vec![source(Some("sources/frcp-fixture.txt"))],
        "Bowles v. Russell, 551 U.S. 205 (2007)",
    );
    let mut diags = vec![];
    let mut push = |severity, code, at: String, message: String| {
        diags.push(Diagnostic {
            severity,
            code,
            at,
            message,
        });
    };
    lint_pack_cites(&p, &frcp_corpus(), &mut push);
    assert!(diags.is_empty());
}

/// End-to-end through the public `lint()` entry point (uses the real
/// embedded corpus via `SourceCorpus::default_source()`), not just the
/// unit-level `lint_pack_cites` helper.
#[test]
fn lint_reports_unverifiable_cite_via_public_entry_point() {
    let p = pack_with_cite(vec![source(Some("sources/frcp.txt"))], "FRCP 9999");
    let g = Graph::compile(
        std::slice::from_ref(&p),
        &LinkFile::default(),
        &CompileOptions::default(),
    )
    .unwrap();
    let diags = lint(&g, std::slice::from_ref(&p));
    assert!(diags.iter().any(|d| d.code == "unverifiable-cite"));
}

fn pack(json: &str) -> Pack {
    Pack::from_json(json).unwrap()
}

#[test]
fn fact_node_without_a_full_prior_is_flagged() {
    let p = pack(
        r#"{
                "schemaVersion": 2, "id": "demo", "title": "Demo", "startNodeId": "check",
                "sources": [{"id": "s", "title": "t", "url": "https://example.com"}],
                "nodes": [
                    {"id": "check", "label": "Check", "kind": "decision", "tags": ["fact"]},
                    {"id": "a", "label": "A", "kind": "terminal", "payoff": 1.0, "outcome": ["win"]},
                    {"id": "b", "label": "B", "kind": "terminal", "payoff": 0.0, "outcome": ["loss"]}
                ],
                "edges": [
                    {"from": "check", "to": "a", "label": "a", "actor": "office"},
                    {"from": "check", "to": "b", "label": "b", "actor": "office", "probability": 0.1}
                ]
            }"#,
    );
    let g = Graph::compile(
        std::slice::from_ref(&p),
        &LinkFile::default(),
        &CompileOptions::default(),
    )
    .unwrap();
    let d = lint(&g, &[p]);
    assert!(d
        .iter()
        .any(|x| x.code == "fact-no-prior" && x.at == "demo::check"));
    assert!(!d.iter().any(|x| x.code == "chance-unquantified"));
}

#[test]
fn fact_node_with_a_full_prior_is_not_flagged() {
    let p = pack(
        r#"{
                "schemaVersion": 2, "id": "demo", "title": "Demo", "startNodeId": "check",
                "sources": [{"id": "s", "title": "t", "url": "https://example.com"}],
                "nodes": [
                    {"id": "check", "label": "Check", "kind": "decision", "tags": ["fact"]},
                    {"id": "a", "label": "A", "kind": "terminal", "payoff": 1.0, "outcome": ["win"]},
                    {"id": "b", "label": "B", "kind": "terminal", "payoff": 0.0, "outcome": ["loss"]}
                ],
                "edges": [
                    {"from": "check", "to": "a", "label": "a", "actor": "office", "probability": 0.9},
                    {"from": "check", "to": "b", "label": "b", "actor": "office", "probability": 0.1}
                ]
            }"#,
    );
    let g = Graph::compile(
        std::slice::from_ref(&p),
        &LinkFile::default(),
        &CompileOptions::default(),
    )
    .unwrap();
    let d = lint(&g, &[p]);
    assert!(!d.iter().any(|x| x.code == "fact-no-prior"));
}

/// The `fact-no-prior` check is gated on "every out-edge is non-applicant"
/// for a pure chance node -- a *mixed* fact node (an applicant choice plus
/// fact-driven interrupts, e.g. ptab's petition-threshold-review) never hit
/// that gate at all before this test, so an unauthored interrupt prior went
/// unflagged.
#[test]
fn mixed_fact_node_without_a_full_interrupt_prior_is_flagged() {
    let p = pack(
        r#"{
                "schemaVersion": 2, "id": "demo", "title": "Demo", "startNodeId": "check",
                "sources": [{"id": "s", "title": "t", "url": "https://example.com"}],
                "nodes": [
                    {"id": "check", "label": "Check", "kind": "decision", "tags": ["fact"]},
                    {"id": "refiled", "label": "Refiled", "kind": "state"},
                    {"id": "a", "label": "A", "kind": "terminal", "payoff": 1.0, "outcome": ["win"]},
                    {"id": "b", "label": "B", "kind": "terminal", "payoff": 0.0, "outcome": ["loss"]}
                ],
                "edges": [
                    {"from": "check", "to": "refiled", "label": "refile", "actor": "applicant"},
                    {"from": "check", "to": "a", "label": "a", "actor": "office"},
                    {"from": "check", "to": "b", "label": "b", "actor": "office", "probability": 0.1}
                ]
            }"#,
    );
    let g = Graph::compile(
        std::slice::from_ref(&p),
        &LinkFile::default(),
        &CompileOptions::default(),
    )
    .unwrap();
    let d = lint(&g, &[p]);
    assert!(d
        .iter()
        .any(|x| x.code == "fact-no-prior" && x.at == "demo::check"));
    // Still a mixed node -- that info diagnostic is orthogonal and unchanged.
    assert!(d
        .iter()
        .any(|x| x.code == "mixed-node" && x.at == "demo::check"));
}

#[test]
fn mixed_fact_node_with_a_full_interrupt_prior_is_not_flagged() {
    let p = pack(
        r#"{
                "schemaVersion": 2, "id": "demo", "title": "Demo", "startNodeId": "check",
                "sources": [{"id": "s", "title": "t", "url": "https://example.com"}],
                "nodes": [
                    {"id": "check", "label": "Check", "kind": "decision", "tags": ["fact"]},
                    {"id": "refiled", "label": "Refiled", "kind": "state"},
                    {"id": "a", "label": "A", "kind": "terminal", "payoff": 1.0, "outcome": ["win"]},
                    {"id": "b", "label": "B", "kind": "terminal", "payoff": 0.0, "outcome": ["loss"]}
                ],
                "edges": [
                    {"from": "check", "to": "refiled", "label": "refile", "actor": "applicant"},
                    {"from": "check", "to": "a", "label": "a", "actor": "office", "probability": 0.9},
                    {"from": "check", "to": "b", "label": "b", "actor": "office", "probability": 0.1}
                ]
            }"#,
    );
    let g = Graph::compile(
        std::slice::from_ref(&p),
        &LinkFile::default(),
        &CompileOptions::default(),
    )
    .unwrap();
    let d = lint(&g, &[p]);
    assert!(!d.iter().any(|x| x.code == "fact-no-prior"));
}

#[test]
fn a_plain_chance_node_without_the_fact_tag_still_gets_chance_unquantified() {
    let p = pack(
        r#"{
                "schemaVersion": 2, "id": "demo", "title": "Demo", "startNodeId": "check",
                "sources": [{"id": "s", "title": "t", "url": "https://example.com"}],
                "nodes": [
                    {"id": "check", "label": "Check", "kind": "decision"},
                    {"id": "a", "label": "A", "kind": "terminal", "payoff": 1.0, "outcome": ["win"]},
                    {"id": "b", "label": "B", "kind": "terminal", "payoff": 0.0, "outcome": ["loss"]}
                ],
                "edges": [
                    {"from": "check", "to": "a", "label": "a", "actor": "office"},
                    {"from": "check", "to": "b", "label": "b", "actor": "office", "probability": 0.1}
                ]
            }"#,
    );
    let g = Graph::compile(
        std::slice::from_ref(&p),
        &LinkFile::default(),
        &CompileOptions::default(),
    )
    .unwrap();
    let d = lint(&g, &[p]);
    assert!(d.iter().any(|x| x.code == "chance-unquantified"));
    assert!(!d.iter().any(|x| x.code == "fact-no-prior"));
}
