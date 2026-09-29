// SPDX-License-Identifier: GPL-3.0-or-later
//! `validate`: check any litgraph document (pack, links, scenario, request)
//! without running an analysis. Parsing is strict (unknown fields are
//! errors); semantic checks compile the graph and resolve the scenario so
//! bad node/edge refs and expressions surface too.

use serde::Serialize;
use serde_json::Value;

use super::catalog::Catalog;
use super::Request;
use crate::error::{Error, Result};
use crate::lint::{self, Diagnostic};
use crate::model::{CompileOptions, Graph, LinkFile, Pack};
use crate::scenario::{Scenario, View};

/// Outcome of validating one document.
#[derive(Debug, Clone, Serialize)]
pub struct Validation {
    /// Detected (or requested) kind.
    pub kind: &'static str,
    /// No errors.
    pub valid: bool,
    /// Errors (parse, reference, expression).
    pub errors: Vec<String>,
    /// Lint diagnostics (packs only).
    pub diagnostics: Vec<Diagnostic>,
    /// Scenario warnings (fallbacks the engine would take).
    pub warnings: Vec<crate::scenario::Warning>,
}

/// Guess a document's kind from its top-level keys.
#[must_use]
pub fn detect(doc: &Value) -> &'static str {
    let has = |k: &str| doc.get(k).is_some();
    if has("schemaVersion") {
        "pack"
    } else if has("links") || has("instances") {
        "links"
    } else if has("op") || has("scenario") || has("packs") {
        "request"
    } else {
        "scenario"
    }
}

fn parse<T: serde::de::DeserializeOwned>(doc: &Value) -> Result<T> {
    serde_json::from_value(doc.clone()).map_err(|e| Error::Parse(e.to_string()))
}

fn scenario_against(
    catalog: &Catalog,
    packs: &[String],
    links: bool,
    sc: &Scenario,
    out: &mut Validation,
) {
    match catalog
        .compile(packs, links, &CompileOptions::default())
        .and_then(|g| View::new(&g, sc).map(|v| v.warnings))
    {
        Ok(w) => out.warnings = w,
        Err(e) => out.errors.push(e.to_string()),
    }
}

/// Validate `doc` as `kind` (or the detected kind).
#[must_use]
pub fn validate(doc: &Value, kind: Option<&str>, catalog: &Catalog) -> Validation {
    let kind = match kind.unwrap_or_else(|| detect(doc)) {
        "pack" => "pack",
        "links" => "links",
        "request" => "request",
        _ => "scenario",
    };
    let mut out = Validation {
        kind,
        valid: false,
        errors: vec![],
        diagnostics: vec![],
        warnings: vec![],
    };
    match kind {
        "pack" => match parse::<Pack>(doc).and_then(|p| {
            Graph::compile(
                std::slice::from_ref(&p),
                &LinkFile::default(),
                &CompileOptions::default(),
            )
            .map(|g| (p, g))
        }) {
            Ok((p, g)) => out.diagnostics = lint::lint(&g, &[p]),
            Err(e) => out.errors.push(e.to_string()),
        },
        "links" => match parse::<LinkFile>(doc) {
            Ok(lf) => {
                let packs: Vec<Pack> = catalog.packs.iter().map(|(_, p, _)| p.clone()).collect();
                if let Err(e) = Graph::compile(&packs, &lf, &CompileOptions::default()) {
                    out.errors.push(e.to_string());
                }
            }
            Err(e) => out.errors.push(e.to_string()),
        },
        "request" => match parse::<Request>(doc) {
            Ok(req) => scenario_against(catalog, &req.packs, req.links, &req.scenario, &mut out),
            Err(e) => out.errors.push(e.to_string()),
        },
        _ => match parse::<Scenario>(doc) {
            Ok(sc) => scenario_against(catalog, &[], true, &sc, &mut out),
            Err(e) => out.errors.push(e.to_string()),
        },
    }
    out.valid = out.errors.is_empty() && !out.diagnostics.iter().any(|d| d.severity == "error");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn detects_and_validates_each_kind() -> Result<()> {
        let c = Catalog::embedded()?;
        let ok = |d: Value| validate(&d, None, &c);
        assert_eq!(ok(json!({"op": {"op": "solve"}})).kind, "request");
        assert!(ok(json!({"packs": ["cofc"], "op": {"op": "solve"}})).valid);
        assert!(!ok(json!({"op": {"op": "solve", "bogus": 1}})).valid);
        assert!(!ok(json!({"params": {"rate": 1}, "remove_edges": ["no-such-edge"]})).valid);
        assert!(ok(json!({"params": {"rate": 1}})).valid);
        assert!(!ok(json!({"cost": "hours * nope"})).valid);
        assert_eq!(ok(json!({"links": []})).kind, "links");
        assert!(ok(json!({"links": []})).valid);
        let pack = json!({"schemaVersion": 2, "id": "t", "title": "t", "startNodeId": "a",
            "nodes": [{"id": "a", "label": "a"}, {"id": "b", "kind": "terminal", "label": "b", "payoff": 1}],
            "edges": [{"from": "a", "to": "b", "label": "go", "actor": "applicant"}]});
        let v = ok(pack.clone());
        assert!(v.kind == "pack" && v.valid, "{:?}", v.errors);
        let mut bad = pack;
        bad["nodes"][0]["colour"] = json!("red");
        assert!(!ok(bad).valid);
        Ok(())
    }

    /// A structurally valid `links.json` (parses fine) whose `replaces`
    /// points at a non-existent edge fails at `Graph::compile`, not parse.
    #[test]
    fn links_compile_error_is_reported_without_panicking() -> Result<()> {
        let c = Catalog::embedded()?;
        let doc = json!({
            "links": [
                {
                    "from": "cofc::claim-accrues",
                    "to": "cofc::dismissed-time-barred",
                    "label": "bogus link",
                    "replaces": ["cofc::no-such-edge"]
                }
            ]
        });
        let v = validate(&doc, Some("links"), &c);
        assert_eq!(v.kind, "links");
        assert!(!v.valid);
        assert!(
            v.errors.iter().any(|e| e.contains("no-such-edge")),
            "{:?}",
            v.errors
        );
        Ok(())
    }

    /// A `links.json` that fails to parse at all (missing required fields)
    /// is reported as a parse error, distinct from the compile-error case
    /// above.
    #[test]
    fn links_parse_error_is_reported() -> Result<()> {
        let c = Catalog::embedded()?;
        // `RawEdge` requires `to` and `label`; only `from` is given.
        let doc = json!({ "links": [ { "from": "cofc::claim-accrues" } ] });
        let v = validate(&doc, Some("links"), &c);
        assert_eq!(v.kind, "links");
        assert!(!v.valid);
        assert_eq!(v.errors.len(), 1);
        Ok(())
    }

    /// A scenario document that fails to parse (unknown field, `deny_unknown_fields`)
    /// is reported as a parse error on the default ("scenario") kind.
    #[test]
    fn scenario_parse_error_is_reported() -> Result<()> {
        let c = Catalog::embedded()?;
        let doc = json!({ "not_a_real_scenario_field": 1 });
        let v = validate(&doc, None, &c);
        assert_eq!(v.kind, "scenario");
        assert!(!v.valid);
        assert_eq!(v.errors.len(), 1);
        Ok(())
    }
}
