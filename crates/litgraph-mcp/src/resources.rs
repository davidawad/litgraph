// SPDX-License-Identifier: GPL-3.0-or-later
//! MCP resources: the manual, `links.json`, every loaded pack, and the
//! named scenario library, served under `litgraph://…` URIs.
//!
//! [`list`] and [`read`] are the only two entry points, each a flat match
//! over the URI scheme. Adding a new resource kind is one new constant,
//! one push in [`list`], and one match/`strip_prefix` arm in [`read`];
//! nothing else in this server needs to change (the scenario resources
//! below, added in a follow-up to the initial packs/links/describe set,
//! are the worked example).

use litgraph::api::{self, Catalog};
use rmcp::model::{Resource, ResourceContents};
use serde_json::{json, to_string_pretty};

/// The manual (same document as the `describe` op / tool).
const DESCRIBE_URI: &str = "litgraph://describe";
/// Cross-pack `links.json`.
const LINKS_URI: &str = "litgraph://links";
/// Prefix for one-pack-per-resource URIs; `<id>` is a pack's own `id`.
const PACK_PREFIX: &str = "litgraph://packs/";
/// Index of every named scenario in the library (id, summary, packs).
const SCENARIOS_INDEX_URI: &str = "litgraph://scenarios";
/// Prefix for one-scenario-per-resource URIs; `<id>` is a scenario's own
/// `id` (or the file stem it was loaded from -- see `Catalog::scenario`).
const SCENARIO_PREFIX: &str = "litgraph://scenarios/";

fn pack_uri(id: &str) -> String {
    format!("{PACK_PREFIX}{id}")
}

fn scenario_uri(id: &str) -> String {
    format!("{SCENARIO_PREFIX}{id}")
}

/// Every resource this server currently exposes.
#[must_use]
pub fn list(catalog: &Catalog) -> Vec<Resource> {
    let mut out = vec![
        Resource::new(DESCRIBE_URI, "describe")
            .with_description(
                "The complete machine-readable manual: ops, scenario fields, metrics, \
                 variables, functions, params -- the same document as the `describe` op.",
            )
            .with_mime_type("application/json"),
        Resource::new(LINKS_URI, "links")
            .with_description("Cross-pack links.json: where forums meet.")
            .with_mime_type("application/json"),
        Resource::new(SCENARIOS_INDEX_URI, "scenarios")
            .with_description(
                "Every named scenario in the library: id, summary, packs it needs. \
                 Read litgraph://scenarios/<id> for the full matter profile.",
            )
            .with_mime_type("application/json"),
    ];
    for (_, pack, _) in &catalog.packs {
        out.push(
            Resource::new(pack_uri(&pack.id), pack.id.clone())
                .with_description(pack.title.clone())
                .with_mime_type("application/json"),
        );
    }
    for scenario in catalog.scenarios() {
        out.push(
            Resource::new(scenario_uri(&scenario.id), scenario.id.clone())
                .with_description(scenario.summary.clone())
                .with_mime_type("application/json"),
        );
    }
    out
}

fn text_resource(uri: &str, body: &str) -> ResourceContents {
    ResourceContents::text(body, uri).with_mime_type("application/json")
}

fn scenarios_index(catalog: &Catalog) -> serde_json::Value {
    json!({
        "origin": catalog.scenarios_origin,
        "scenarios": catalog.scenarios().map(|s| json!({
            "id": s.id,
            "summary": s.summary,
            "packs": s.packs,
        })).collect::<Vec<_>>(),
    })
}

/// Resolve one resource URI to its contents, or `None` if this server has
/// no resource at that URI.
#[must_use]
pub fn read(catalog: &Catalog, uri: &str) -> Option<Vec<ResourceContents>> {
    if uri == DESCRIBE_URI {
        return Some(vec![text_resource(
            uri,
            &api::describe(catalog).to_string(),
        )]);
    }
    if uri == LINKS_URI {
        let body = to_string_pretty(&catalog.links).ok()?;
        return Some(vec![text_resource(uri, &body)]);
    }
    if uri == SCENARIOS_INDEX_URI {
        let body = to_string_pretty(&scenarios_index(catalog)).ok()?;
        return Some(vec![text_resource(uri, &body)]);
    }
    if let Some(id) = uri.strip_prefix(PACK_PREFIX) {
        let (_, pack, _) = catalog.packs.iter().find(|(_, p, _)| p.id == id)?;
        let body = to_string_pretty(pack).ok()?;
        return Some(vec![text_resource(uri, &body)]);
    }
    if let Some(id) = uri.strip_prefix(SCENARIO_PREFIX) {
        let named = catalog.scenario(id).ok()?;
        let body = to_string_pretty(named).ok()?;
        return Some(vec![text_resource(uri, &body)]);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> Catalog {
        Catalog::embedded().expect("embedded packs must load in tests")
    }

    #[test]
    fn list_includes_describe_links_every_pack_and_every_scenario() {
        let c = catalog();
        assert!(
            !c.scenarios.is_empty(),
            "embedded catalog should ship named scenarios"
        );
        let uris: Vec<String> = list(&c).into_iter().map(|r| r.uri).collect();
        assert!(uris.contains(&DESCRIBE_URI.to_string()));
        assert!(uris.contains(&LINKS_URI.to_string()));
        assert!(uris.contains(&SCENARIOS_INDEX_URI.to_string()));
        for (_, pack, _) in &c.packs {
            assert!(
                uris.contains(&pack_uri(&pack.id)),
                "missing resource for pack {}",
                pack.id
            );
        }
        for scenario in c.scenarios() {
            assert!(
                uris.contains(&scenario_uri(&scenario.id)),
                "missing resource for scenario {}",
                scenario.id
            );
        }
        assert_eq!(uris.len(), 3 + c.packs.len() + c.scenarios.len());
    }

    #[test]
    fn read_describe_is_the_same_document_as_the_op() {
        let c = catalog();
        let contents = read(&c, DESCRIBE_URI).unwrap();
        let ResourceContents::TextResourceContents { text, .. } = &contents[0] else {
            panic!("expected text contents");
        };
        let parsed: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed, api::describe(&c));
    }

    #[test]
    fn read_links_round_trips_as_json() {
        let c = catalog();
        let contents = read(&c, LINKS_URI).unwrap();
        let ResourceContents::TextResourceContents { text, .. } = &contents[0] else {
            panic!("expected text contents");
        };
        assert!(serde_json::from_str::<serde_json::Value>(text).is_ok());
    }

    #[test]
    fn read_a_pack_returns_that_packs_json() {
        let c = catalog();
        let (_, pack, _) = &c.packs[0];
        let contents = read(&c, &pack_uri(&pack.id)).unwrap();
        let ResourceContents::TextResourceContents { text, .. } = &contents[0] else {
            panic!("expected text contents");
        };
        let parsed: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["id"], serde_json::json!(pack.id));
    }

    #[test]
    fn read_scenarios_index_lists_every_scenario_by_id_summary_and_packs() {
        let c = catalog();
        let contents = read(&c, SCENARIOS_INDEX_URI).unwrap();
        let ResourceContents::TextResourceContents { text, .. } = &contents[0] else {
            panic!("expected text contents");
        };
        let parsed: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["origin"], serde_json::json!(c.scenarios_origin));
        let listed = parsed["scenarios"].as_array().unwrap();
        assert_eq!(listed.len(), c.scenarios.len());
        let first = c.scenarios().next().expect("at least one scenario");
        let entry = listed
            .iter()
            .find(|e| e["id"] == serde_json::json!(first.id))
            .unwrap();
        assert_eq!(entry["summary"], serde_json::json!(first.summary));
        assert_eq!(entry["packs"], serde_json::json!(first.packs));
    }

    #[test]
    fn read_a_scenario_returns_the_full_named_scenario_json() {
        let c = catalog();
        let named = c.scenarios().next().expect("at least one scenario");
        let contents = read(&c, &scenario_uri(&named.id)).unwrap();
        let ResourceContents::TextResourceContents { text, .. } = &contents[0] else {
            panic!("expected text contents");
        };
        let parsed: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["id"], serde_json::json!(named.id));
        assert_eq!(parsed["summary"], serde_json::json!(named.summary));
        assert!(
            parsed.get("scenario").is_some(),
            "expected the full engine Scenario embedded"
        );
    }

    #[test]
    fn read_returns_none_for_an_unknown_scenario_id() {
        let c = catalog();
        assert!(read(&c, &scenario_uri("no-such-scenario")).is_none());
    }

    #[test]
    fn read_returns_none_for_an_unknown_uri() {
        let c = catalog();
        assert!(read(&c, "litgraph://packs/does-not-exist").is_none());
        assert!(read(&c, "litgraph://nope").is_none());
    }
}
