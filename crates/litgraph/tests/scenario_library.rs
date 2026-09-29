// SPDX-License-Identifier: GPL-3.0-or-later
//! Every shipped scenario (`scenarios/*.json`, embedded via `Catalog::embedded`)
//! must resolve against the *current* embedded packs: its `packs` compile,
//! its `scenario` resolves into a [`View`] without error, `start` (if set)
//! resolves to a real node, and every qualified node/edge ref in `payoffs` /
//! `remove_edges` / `probabilities` / `policy` resolves. This is the "lint or
//! test that every shipped scenario validates against the current packs"
//! called for by the scenario-library feature; a future pack edit that
//! renames/removes a node or edge a scenario depends on fails this test
//! immediately instead of silently drifting.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use litgraph::api::Catalog;
use litgraph::model::CompileOptions;
use litgraph::scenario::View;

#[test]
fn every_shipped_scenario_has_a_summary_and_at_least_one_pack() {
    let catalog = Catalog::embedded().expect("embedded catalog compiles");
    for def in catalog.scenarios() {
        assert!(!def.summary.trim().is_empty(), "{}: empty summary", def.id);
        assert!(!def.packs.is_empty(), "{}: names no packs", def.id);
    }
}

#[test]
fn every_shipped_scenario_resolves_against_the_current_packs() {
    let catalog = Catalog::embedded().expect("embedded catalog compiles");
    let mut checked = 0;
    for def in catalog.scenarios() {
        let g = catalog
            .compile(&def.packs, true, &CompileOptions::default())
            .unwrap_or_else(|e| panic!("{}: packs {:?} failed to compile: {e}", def.id, def.packs));
        let view = View::new(&g, &def.scenario)
            .unwrap_or_else(|e| panic!("{}: scenario failed to resolve: {e}", def.id));
        // A scenario that silently resolves to a single unreachable/sink
        // start would be a content bug, not a passing "it parsed" result.
        assert!(
            view.g.out[view.start].iter().any(|&e| view.active[e])
                || view.g.nodes[view.start].kind == litgraph::model::NodeKind::Terminal,
            "{}: start node {} has no active out-edge and is not a terminal",
            def.id,
            view.g.nodes[view.start].id
        );
        checked += 1;
    }
    assert!(
        checked >= 4,
        "expected at least 4 shipped scenarios, found {checked}"
    );
}

/// Every named scenario is independently discoverable and loadable through
/// the public `Catalog::scenario` lookup lg-mcp is expected to build on.
#[test]
fn every_shipped_scenario_is_individually_loadable_by_id() {
    let catalog = Catalog::embedded().expect("embedded catalog compiles");
    let ids: Vec<String> = catalog.scenarios().map(|s| s.id.clone()).collect();
    for id in ids {
        let def = catalog.scenario(&id).expect("loadable by its own id");
        assert_eq!(def.id, id);
    }
}

/// `validate` (the CLI/API surface) accepts a scenario-library file directly,
/// resolving it against the embedded packs it names.
#[test]
fn validate_accepts_a_scenario_library_file() {
    let catalog = Catalog::embedded().expect("embedded catalog compiles");
    for (name, def, _) in &catalog.scenarios {
        let doc: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../scenarios")
                    .join(name),
            )
            .unwrap_or_else(|e| panic!("reading scenarios/{name}: {e}")),
        )
        .unwrap_or_else(|e| panic!("scenarios/{name} is not JSON: {e}"));
        let v = litgraph::api::validate(&doc, None, &catalog);
        assert_eq!(v.kind, "named-scenario", "{}: {:?}", def.id, v);
        assert!(v.valid, "{}: {:?}", def.id, v.errors);
    }
}
