// SPDX-License-Identifier: GPL-3.0-or-later
//! WebAssembly bindings for litgraph.
//!
//! The contract is identical to the CLI and the Rust API: JSON strings in,
//! JSON strings out, errors reported inside the response (`ok: false`), never
//! thrown. Packs are embedded; pass your own with [`handle_with_packs`].
//!
//! ```js
//! import init, { handle, describe } from "litgraph-wasm";
//! await init();
//! const resp = JSON.parse(handle(JSON.stringify({ packs: ["cofc"], op: { op: "solve" } })));
//! ```

use std::sync::OnceLock;

use litgraph::api::{self, Catalog};
use serde_json::{json, Value};
use wasm_bindgen::prelude::wasm_bindgen;

fn catalog() -> Result<&'static Catalog, String> {
    static CATALOG: OnceLock<Result<Catalog, String>> = OnceLock::new();
    CATALOG
        .get_or_init(|| Catalog::embedded().map_err(|e| e.to_string()))
        .as_ref()
        .map_err(Clone::clone)
}

fn error(code: &str, message: &str) -> String {
    json!({ "ok": false, "api_version": api::API_VERSION, "op": "unknown", "warnings": [], "error": { "code": code, "message": message, "hint": "" }, "elapsed_ms": 0.0 }).to_string()
}

fn to_string(v: &impl serde::Serialize) -> String {
    serde_json::to_string(v).unwrap_or_else(|e| error("invalid", &e.to_string()))
}

/// Run a request (JSON string) against the embedded packs; returns the response JSON.
#[wasm_bindgen]
#[must_use]
pub fn handle(request_json: &str) -> String {
    match catalog() {
        Ok(c) => to_string(&api::handle_json(request_json, c)),
        Err(e) => error("parse", &e),
    }
}

/// Run a request against caller-supplied packs: `packs_json` is an object
/// mapping file names to pack documents, e.g. `{"my.json": {...}, "links.json": {...}}`.
#[wasm_bindgen]
#[must_use]
pub fn handle_with_packs(request_json: &str, packs_json: &str) -> String {
    let files: serde_json::Map<String, Value> = match serde_json::from_str(packs_json) {
        Ok(Value::Object(m)) => m,
        Ok(_) => return error("parse", "packs must be an object of {file name: pack}"),
        Err(e) => return error("parse", &e.to_string()),
    };
    match Catalog::from_files(
        "inline".into(),
        files.into_iter().map(|(k, v)| (k, v.to_string())),
    ) {
        Ok(c) => to_string(&api::handle_json(request_json, &c)),
        Err(e) => error("parse", &e.to_string()),
    }
}

/// Validate a document (pack, links, scenario or request); `kind` forces the kind.
// `wasm_bindgen`'s JS-interop ABI needs owned `Option<String>` here, not `Option<&str>`.
#[allow(clippy::needless_pass_by_value)]
#[wasm_bindgen]
#[must_use]
pub fn validate(doc_json: &str, kind: Option<String>) -> String {
    let c = match catalog() {
        Ok(c) => c,
        Err(e) => return error("parse", &e),
    };
    match serde_json::from_str::<Value>(doc_json) {
        Ok(doc) => to_string(&api::validate(&doc, kind.as_deref(), c)),
        Err(e) => error("parse", &e.to_string()),
    }
}

/// JSON Schema for `request`, `response`, `scenario`, `pack` or `links`.
#[wasm_bindgen]
#[must_use]
pub fn schema(kind: &str) -> String {
    match api::schema(kind) {
        Ok(s) => s.to_string(),
        Err(e) => error(e.code(), &e.to_string()),
    }
}

/// The machine-readable manual.
#[wasm_bindgen]
#[must_use]
pub fn describe() -> String {
    handle(r#"{"op":{"op":"describe"}}"#)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Value {
        serde_json::from_str(s).unwrap_or(Value::Null)
    }

    #[test]
    fn same_contract_as_the_cli() {
        let r = parse(&handle(r#"{"packs":["cofc"],"op":{"op":"solve"}}"#));
        assert_eq!(r["ok"], json!(true));
        assert!(r["result"]["value"].is_number());
        assert_eq!(parse(&handle("not json"))["ok"], json!(false));
        assert_eq!(parse(&describe())["op"], json!("describe"));
        assert!(parse(&schema("scenario"))["title"].is_string());
        assert_eq!(parse(&schema("nope"))["ok"], json!(false));
        assert_eq!(
            parse(&validate(r#"{"params":{"rate":1}}"#, None))["valid"],
            json!(true)
        );
        assert_eq!(parse(&validate("{", None))["ok"], json!(false));
    }

    #[test]
    fn inline_packs() {
        let pack = json!({"schemaVersion": 2, "id": "t", "title": "t", "startNodeId": "a",
            "nodes": [{"id": "a", "label": "a"}, {"id": "b", "kind": "terminal", "label": "b", "payoff": 5}],
            "edges": [{"from": "a", "to": "b", "label": "go", "actor": "applicant", "hours": 0}]});
        let packs = json!({ "t.json": pack }).to_string();
        let r = parse(&handle_with_packs(r#"{"op":{"op":"solve"}}"#, &packs));
        assert_eq!(r["result"]["value"], json!(5.0));
        assert_eq!(parse(&handle_with_packs("{}", "[]"))["ok"], json!(false));
        assert_eq!(parse(&handle_with_packs("{}", "{"))["ok"], json!(false));
    }
}
