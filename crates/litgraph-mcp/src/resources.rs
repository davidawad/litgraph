// SPDX-License-Identifier: GPL-3.0-or-later
//! MCP resources: the manual, `links.json`, and every loaded pack, served
//! under `litgraph://…` URIs.
//!
//! [`list`] and [`read`] are the only two entry points, each a flat match
//! over the URI scheme. Adding a new resource kind — in particular
//! `litgraph://scenarios/<name>` once the scenario library (lg-gl4,
//! `scenarios/*.json`) exists — is one new constant, one push in [`list`],
//! and one `strip_prefix` arm in [`read`]; nothing else in this server
//! needs to change. That wiring is filed as follow-up bead lg-3cx rather
//! than stubbed here, since there is nothing to load yet.

use litgraph::api::{self, Catalog};
use rmcp::model::{Resource, ResourceContents};
use serde_json::to_string_pretty;

/// The manual (same document as the `describe` op / tool).
const DESCRIBE_URI: &str = "litgraph://describe";
/// Cross-pack `links.json`.
const LINKS_URI: &str = "litgraph://links";
/// Prefix for one-pack-per-resource URIs; `<id>` is a pack's own `id`.
const PACK_PREFIX: &str = "litgraph://packs/";

fn pack_uri(id: &str) -> String {
    format!("{PACK_PREFIX}{id}")
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
    ];
    for (_, pack, _) in &catalog.packs {
        out.push(
            Resource::new(pack_uri(&pack.id), pack.id.clone())
                .with_description(pack.title.clone())
                .with_mime_type("application/json"),
        );
    }
    out
}

fn text_resource(uri: &str, body: &str) -> ResourceContents {
    ResourceContents::text(body, uri).with_mime_type("application/json")
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
    if let Some(id) = uri.strip_prefix(PACK_PREFIX) {
        let (_, pack, _) = catalog.packs.iter().find(|(_, p, _)| p.id == id)?;
        let body = to_string_pretty(pack).ok()?;
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
    fn list_includes_describe_links_and_every_pack() {
        let c = catalog();
        let uris: Vec<String> = list(&c).into_iter().map(|r| r.uri).collect();
        assert!(uris.contains(&DESCRIBE_URI.to_string()));
        assert!(uris.contains(&LINKS_URI.to_string()));
        for (_, pack, _) in &c.packs {
            assert!(
                uris.contains(&pack_uri(&pack.id)),
                "missing resource for pack {}",
                pack.id
            );
        }
        assert_eq!(uris.len(), 2 + c.packs.len());
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
    fn read_returns_none_for_an_unknown_uri() {
        let c = catalog();
        assert!(read(&c, "litgraph://packs/does-not-exist").is_none());
        assert!(read(&c, "litgraph://nope").is_none());
    }
}
