// SPDX-License-Identifier: GPL-3.0-or-later
//! Targeted tests for branches the other suites don't happen to exercise:
//! id-resolution ambiguity/near-matches, catalog loading from a directory,
//! every `lint` diagnostic code, chain edge cases (terminal start, singular
//! systems), the less-common edge/terminal metric variables and functions,
//! and Monte Carlo's sampled-duration path.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use litgraph::api::{Catalog, Response};
use litgraph::algo::{chain, mdp, sim};
use litgraph::model::{CompileOptions, Graph, LinkFile, Pack};
use litgraph::scenario::{Scenario, View};
use litgraph::{lint, Result};
use serde_json::json;
use std::collections::BTreeMap;

fn pack(json: serde_json::Value) -> Pack {
    serde_json::from_value(json).expect("test pack literal is well-formed")
}

fn compile(packs: &[Pack]) -> Graph {
    Graph::compile(packs, &LinkFile::default(), &CompileOptions::default()).expect("test packs compile")
}

fn view(g: &Graph, sc: &Scenario) -> View<'_> {
    View::new(g, sc).expect("test scenario resolves")
}

// --- model::resolve: ambiguity, near-match suggestions, edge_at fallback ---

fn two_pack_dup_ids() -> Graph {
    let p1 = pack(json!({
        "schemaVersion": 2, "id": "p1", "title": "p1", "startNodeId": "dup",
        "nodes": [
            { "id": "dup", "kind": "state", "label": "dup in p1" },
            { "id": "end1", "kind": "terminal", "label": "end1", "payoff": 1 }
        ],
        "edges": [
            { "id": "dup-edge", "from": "dup", "to": "end1", "label": "go", "actor": "either" }
        ]
    }));
    let p2 = pack(json!({
        "schemaVersion": 2, "id": "p2", "title": "p2", "startNodeId": "dup",
        "nodes": [
            { "id": "dup", "kind": "state", "label": "dup in p2" },
            { "id": "end2", "kind": "terminal", "label": "end2", "payoff": 1 }
        ],
        "edges": [
            { "id": "dup-edge", "from": "dup", "to": "end2", "label": "go", "actor": "either" }
        ]
    }));
    compile(&[p1, p2])
}

#[test]
fn ambiguous_local_node_id_is_invalid() {
    let g = two_pack_dup_ids();
    let err = g.node("dup").unwrap_err();
    assert_eq!(err.code(), "invalid");
    assert!(err.to_string().contains("ambiguous"));
    // Qualified ids still resolve unambiguously.
    assert!(g.node("p1::dup").is_ok());
    assert!(g.node("p2::dup").is_ok());
}

#[test]
fn ambiguous_local_edge_id_is_invalid() {
    let g = two_pack_dup_ids();
    let err = g.edge("dup-edge").unwrap_err();
    assert_eq!(err.code(), "invalid");
    assert!(err.to_string().contains("ambiguous"));
    assert!(g.edge("p1::dup-edge").is_ok());
}

#[test]
fn unknown_node_id_suggests_near_matches_when_any_exist() {
    let g = two_pack_dup_ids();
    // "dup" is a substring of the qualified id "p1::dup" -> suggestion branch.
    let err = g.node("nonexistent-but-dup-ish").unwrap_err();
    assert_eq!(err.code(), "not-found");

    // A totally unrelated query hits the empty-suggestion branch instead.
    let err = g.node("zzz-totally-unrelated-000").unwrap_err();
    assert_eq!(err.code(), "not-found");
    assert!(!err.to_string().contains("did you mean"));
}

#[test]
fn edge_at_resolves_by_label_and_reports_options_when_not_unique() {
    let g = two_pack_dup_ids();
    let n = g.node("p1::dup").unwrap();
    // "go" is the edge's label, not its id -> falls through to the label search.
    let e = g.edge_at(n, "go").unwrap();
    assert_eq!(g.edges[e].label, "go");

    let err = g.edge_at(n, "no-such-label").unwrap_err();
    assert_eq!(err.code(), "not-found");
    assert!(err.to_string().contains("options:"));
}

// --- api::catalog: loading a directory, and a path-based pack ref ---

#[test]
fn catalog_loads_from_a_directory_with_links() -> Result<()> {
    let dir = std::env::temp_dir().join(format!("litgraph-cov-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| litgraph::Error::Io(e.to_string()))?;
    let pack_path = dir.join("a.json");
    std::fs::write(
        &pack_path,
        json!({
            "schemaVersion": 2, "id": "a", "title": "a", "startNodeId": "s",
            "nodes": [
                { "id": "s", "kind": "state", "label": "s" },
                { "id": "e", "kind": "terminal", "label": "e", "payoff": 1 }
            ],
            "edges": [{ "id": "go", "from": "s", "to": "e", "label": "go", "actor": "either" }]
        })
        .to_string(),
    )
    .map_err(|e| litgraph::Error::Io(e.to_string()))?;
    std::fs::write(dir.join("links.json"), json!({ "links": [], "instances": {} }).to_string())
        .map_err(|e| litgraph::Error::Io(e.to_string()))?;
    // A non-JSON file in the directory must be ignored, not error.
    std::fs::write(dir.join("README.md"), "not a pack").map_err(|e| litgraph::Error::Io(e.to_string()))?;

    let cat = Catalog::load(&dir)?;
    assert_eq!(cat.packs.len(), 1);
    assert_eq!(cat.origin, dir.display().to_string());
    let g = cat.compile(&[], true, &CompileOptions::default())?;
    assert_eq!(g.nodes.len(), 2);

    // A pack ref that is a path to an existing .json file (not a loaded id).
    let (p, fp) = cat.select(&[pack_path.display().to_string()])?.into_iter().next().unwrap();
    assert_eq!(p.id, "a");
    assert!(!fp.is_empty());

    std::fs::remove_dir_all(&dir).ok();
    Ok(())
}

#[test]
fn catalog_load_of_missing_directory_is_io_error() {
    let err = Catalog::load(std::path::Path::new("/does/not/exist/at/all")).unwrap_err();
    assert_eq!(err.code(), "io");
}

#[test]
fn default_source_falls_back_to_embedded_without_the_env_var() {
    // `LITGRAPH_PACKS` is not set in the test process, so this exercises the
    // `None => embedded()` branch (the `Some` branch needs `set_var`, which
    // is `unsafe` and this workspace forbids `unsafe_code` even in tests).
    assert!(std::env::var_os("LITGRAPH_PACKS").is_none());
    let cat = Catalog::default_source().unwrap();
    assert_eq!(cat.origin, "embedded");
}

// --- lint: every diagnostic code ---

#[test]
fn lint_covers_every_diagnostic_code() {
    let p = pack(json!({
        "schemaVersion": 1, "id": "l", "title": "l", "startNodeId": "start",
        "sources": [{ "id": "bad-src", "path": "/etc/passwd" }],
        "nodes": [
            { "id": "start", "kind": "state", "label": "start" },
            { "id": "choice-with-prob", "kind": "decision", "label": "choice-with-prob" },
            { "id": "term-no-payoff", "kind": "terminal", "label": "term-no-payoff" },
            { "id": "term-no-outcome", "kind": "terminal", "label": "term-no-outcome", "payoff": 1 },
            { "id": "dead-end", "kind": "state", "label": "dead-end" },
            { "id": "unquantified", "kind": "state", "label": "unquantified" },
            { "id": "mixed-node", "kind": "state", "label": "mixed-node" },
            { "id": "unreachable-node", "kind": "state", "label": "unreachable-node" },
            { "id": "a", "kind": "terminal", "label": "a", "payoff": 1, "outcome": ["x"] },
            { "id": "b", "kind": "terminal", "label": "b", "payoff": 1, "outcome": ["x"] },
            { "id": "bad-dl", "kind": "terminal", "label": "bad-dl", "payoff": 1 },
            { "id": "bad-dur", "kind": "terminal", "label": "bad-dur", "payoff": 1 }
        ],
        "edges": [
            { "from": "start", "to": "choice-with-prob", "label": "step", "actor": "either" },
            { "from": "choice-with-prob", "to": "a", "label": "applicant-move", "actor": "applicant", "probability": 0.5 },
            { "from": "choice-with-prob", "to": "b", "label": "applicant-move2", "actor": "applicant" },
            { "from": "start", "to": "unquantified", "label": "to-unquantified", "actor": "either" },
            { "from": "unquantified", "to": "a", "label": "u1", "actor": "either", "probability": 0.9 },
            { "from": "unquantified", "to": "b", "label": "u2", "actor": "either" },
            { "from": "start", "to": "mixed-node", "label": "to-mixed", "actor": "either" },
            { "from": "mixed-node", "to": "a", "label": "mixed-self", "actor": "applicant" },
            { "from": "mixed-node", "to": "b", "label": "mixed-world", "actor": "either", "probability": 1.0 },
            { "from": "start", "to": "term-no-payoff", "label": "to-npo", "actor": "either" },
            { "from": "start", "to": "term-no-outcome", "label": "to-nout", "actor": "either" },
            { "from": "term-no-outcome", "to": "term-no-outcome", "label": "no-authority-edge", "actor": "applicant" },
            { "from": "start", "to": "bad-dl", "label": "bad-deadline-edge", "actor": "either", "deadline": { "length": 0 } },
            { "from": "start", "to": "bad-dur", "label": "bad-duration-edge", "actor": "either", "duration": { "min": 10, "mode": 5 } },
            { "from": "a", "to": "a", "label": "parallel-1", "actor": "either" },
            { "from": "a", "to": "a", "label": "parallel-2", "actor": "either" }
        ]
    }));
    let g = compile(&[p.clone()]);
    let diags = lint::lint(&g, &[p]);
    let codes: std::collections::BTreeSet<&str> = diags.iter().map(|d| d.code).collect();
    for want in [
        "schema-v1",
        "local-source-path",
        "no-sources",
        "probability-on-choice",
        "parallel-edge-no-id",
        "no-authority",
        "bad-deadline",
        "bad-duration",
        "terminal-no-payoff",
        "terminal-no-outcome",
        "dead-end",
        "probability-sum",
        "chance-unquantified",
        "mixed-node",
        "unreachable",
    ] {
        assert!(codes.contains(want), "missing lint code {want}; got {codes:?}");
    }
}

#[test]
fn lint_reports_a_pack_start_that_does_not_resolve() {
    // A pack whose own declared start is fine at parse time but is renamed
    // away in the compiled graph never happens in practice (compile itself
    // validates it) — so this exercises the defensive branch directly via a
    // hand-built graph the ordinary compile path can't produce: two packs
    // where one instance's start id was valid for compilation but the pack
    // metadata queried by `lint` is a stale copy with a different start.
    let p = pack(json!({
        "schemaVersion": 2, "id": "s", "title": "s", "startNodeId": "real-start",
        "nodes": [
            { "id": "real-start", "kind": "terminal", "label": "real-start", "payoff": 0 }
        ],
        "edges": []
    }));
    let g = compile(&[p]);
    // A stale pack record whose start doesn't exist in `g` at all.
    let stale = pack(json!({
        "schemaVersion": 2, "id": "s", "title": "s", "startNodeId": "ghost-start",
        "nodes": [{ "id": "real-start", "kind": "terminal", "label": "real-start", "payoff": 0 }],
        "edges": []
    }));
    let diags = lint::lint(&g, &[stale]);
    assert!(diags.iter().any(|d| d.code == "bad-pack-start"), "{diags:?}");
}

// --- algo::chain: terminal start, and a policy that never terminates ---

#[test]
fn chain_from_a_terminal_start_is_trivial() {
    let g = compile(&[pack(json!({
        "schemaVersion": 2, "id": "t", "title": "t", "startNodeId": "only",
        "nodes": [{ "id": "only", "kind": "terminal", "label": "only", "payoff": 42 }],
        "edges": []
    }))]);
    let v = view(&g, &Scenario::default());
    let sol = mdp::solve(&v, &mdp::SolveOptions::default()).unwrap();
    let c = chain::chain(&v, &sol, v.start, &[]).unwrap();
    assert_eq!(c.absorption, vec![(v.start, 1.0)]);
    assert_eq!(c.expected_utility, 42.0);
    assert_eq!(c.visits, vec![]);
}

#[test]
fn chain_reports_a_policy_that_never_terminates() {
    // A self-loop the applicant is forced to keep taking: no terminal is
    // ever reachable under this fixed (non-optimal) policy.
    let g = compile(&[pack(json!({
        "schemaVersion": 2, "id": "loop", "title": "loop", "startNodeId": "s",
        "nodes": [
            { "id": "s", "kind": "state", "label": "s" },
            { "id": "e", "kind": "terminal", "label": "e", "payoff": 1 }
        ],
        "edges": [
            { "id": "spin", "from": "s", "to": "s", "label": "spin", "actor": "applicant" },
            { "id": "exit", "from": "s", "to": "e", "label": "exit", "actor": "applicant" }
        ]
    }))]);
    let v = view(&g, &Scenario::default());
    let mut sol = mdp::solve(&v, &mdp::SolveOptions::default()).unwrap();
    // Override the solved (optimal, exits) policy with a forced infinite spin.
    sol.choice = BTreeMap::from([(v.start, g.edge("spin").unwrap())]);
    let err = chain::chain(&v, &sol, v.start, &[]).unwrap_err();
    assert_eq!(err.code(), "numeric");
    assert!(err.to_string().contains("never terminates"));
}

// --- metrics: the less-common edge/terminal variables and functions ---

#[test]
fn edge_and_terminal_metric_functions_and_attrs() {
    let g = compile(&[pack(json!({
        "schemaVersion": 2, "id": "m", "title": "m", "startNodeId": "s",
        "nodes": [
            { "id": "s", "kind": "state", "label": "S", "attrs": { "risk": 3.0 } },
            { "id": "e", "kind": "terminal", "label": "E", "payoff": 10, "outcome": ["win"], "attrs": { "weight": 5.0 } }
        ],
        "edges": [{
            "id": "go", "from": "s", "to": "e", "label": "Go", "actor": "applicant",
            "authority": "Rule 37", "attrs": { "risk": 2.0 },
            "valence": "good"
        }]
    }))]);
    let v = view(&g, &Scenario::default());

    for (spec, want) in [
        ("tag('nope')", 0.0),
        ("actor('applicant')", 1.0),
        ("pack('m')", 1.0),
        ("label_has('GO')", 1.0),
        ("authority_has('rule 37')", 1.0),
        ("attr('risk', -1)", 2.0),
        ("attr('missing', -1)", -1.0),
        ("edge.risk", 2.0),
        ("from.risk", 3.0),
        ("to.weight", 5.0),
        ("valence_good", 1.0),
        ("valence_bad", 0.0),
        ("valence_caution", 0.0),
        ("is_link", 0.0),
        ("is_synthetic", 0.0),
        ("to_terminal", 1.0),
        ("to_tag('win')", 1.0),
        ("from_tag('win')", 0.0),
    ] {
        let got = v.metric(spec).unwrap()[g.edge("go").unwrap()];
        assert!((got - want).abs() < 1e-9, "{spec}: got {got}, want {want}");
    }

    for (spec, want) in [("tag('win')", 1.0), ("pack('m')", 1.0), ("label_has('e')", 1.0), ("node.weight", 5.0)] {
        let got = v.terminal_metric(spec).unwrap()[g.node("e").unwrap()];
        assert!((got - want).abs() < 1e-9, "{spec}: got {got}, want {want}");
    }
}

// --- algo::sim: sampled durations along the trajectory ---

#[test]
fn simulation_can_sample_durations_from_triangular() {
    let g = compile(&[pack(json!({
        "schemaVersion": 2, "id": "d", "title": "d", "startNodeId": "s",
        "nodes": [
            { "id": "s", "kind": "state", "label": "s" },
            { "id": "e", "kind": "terminal", "label": "e", "payoff": 0 }
        ],
        "edges": [{
            "id": "go", "from": "s", "to": "e", "label": "go", "actor": "applicant",
            "duration": { "min": 10, "mode": 20, "max": 40 }
        }]
    }))]);
    let v = view(&g, &Scenario::default());
    let sol = mdp::solve(&v, &mdp::SolveOptions::default()).unwrap();
    let ms = vec![("elapsed".to_string(), v.metric("elapsed").unwrap())];
    let r = sim::simulate(
        &v,
        &sol,
        v.start,
        &ms,
        &sim::SimOptions { runs: 200, seed: 3, alpha: 0.1, max_steps: 10, sample_durations: true, keep_samples: 1 },
    )
    .unwrap();
    let e = &r.metrics["elapsed"];
    assert!(e.min >= 10.0 - 1e-9 && e.max <= 40.0 + 1e-9, "{e:?}");
    assert_eq!(r.samples.len(), 1);
}

// A response's `Debug` impl is used throughout the api_coverage suite's
// assertion messages; a tiny smoke test keeps that derive from silently
// bit-rotting into an unused-field trap.
#[test]
fn response_is_debuggable() {
    let r = Response {
        ok: true,
        api_version: 1,
        op: "describe".into(),
        result: None,
        warnings: vec![],
        provenance: None,
        error: None,
        elapsed_ms: 0.0,
    };
    assert!(format!("{r:?}").contains("describe"));
}
