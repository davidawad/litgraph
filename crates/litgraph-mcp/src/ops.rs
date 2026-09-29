// SPDX-License-Identifier: GPL-3.0-or-later
//! One MCP tool per engine [`Op`](litgraph::api::Op), plus the generic
//! `litgraph` tool that takes a raw [`Request`](litgraph::api::Request).
//!
//! Every tool's input schema is *sliced* out of the engine's own
//! `schemars` schema for `Request` (via `litgraph schema request`'s
//! generator, [`schemars::schema_for!`]) rather than hand-written: the
//! `Op` enum is a serde-internally-tagged `oneOf`, one variant per op, so
//! finding the variant whose `properties.op.const` equals the op name and
//! stripping the `op` tag gives exactly that op's arguments — the same
//! shape `litgraph::api::Op`'s own `Deserialize` accepts, with the shared
//! request-level fields (`packs`, `links`, `no_continuations`, `scenario`)
//! merged in. If a field is added to an `Op` variant, this schema picks it
//! up automatically; nothing here needs editing.
//!
//! Dispatch mirrors this: a tool call's `arguments` are exactly `{packs?,
//! links?, no_continuations?, scenario?, ...op fields}`; [`call_op`]
//! reassembles the full `{packs, links, no_continuations, scenario, op:
//! {op: name, ...}}` envelope and hands it to
//! [`litgraph::api::handle_json`] as text, so parsing, validation, and
//! error shaping are 100% the engine's own — this module never
//! constructs an [`litgraph::api::Op`] or [`litgraph::api::Request`] by hand.

use std::sync::OnceLock;

use litgraph::api::{self, Catalog, Response};
use rmcp::model::{JsonObject, Tool};
use serde_json::{json, Map, Value};

/// Every engine op's wire name, in the engine's own dispatch order
/// (`litgraph::api::Op::name()`'s targets). Kept as a literal list (rather
/// than derived from the schema) so the tool set is visible at a glance;
/// [`tools`] and its tests fail loudly if this ever drifts from the
/// schema's own `oneOf` variants.
pub const OP_NAMES: &[&str] = &[
    "describe",
    "packs",
    "lint",
    "validate",
    "graph",
    "metric",
    "explain",
    "solve",
    "chain",
    "simulate",
    "path",
    "pareto",
    "sweep",
    "tornado",
    "structure",
    "compare",
    "batch",
];

/// Wire name of the generic, raw-`Request` tool.
pub const GENERIC_TOOL: &str = "litgraph";

fn request_schema() -> &'static Value {
    static SCHEMA: OnceLock<Value> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        serde_json::to_value(schemars::schema_for!(api::Request)).unwrap_or_else(|_| json!({}))
    })
}

/// The shared, non-op fields every request carries (`packs`, `links`,
/// `no_continuations`, `scenario`), taken verbatim from the `Request`
/// schema's own `properties`.
fn shared_properties() -> Map<String, Value> {
    let mut out = Map::new();
    let Some(props) = request_schema()
        .get("properties")
        .and_then(Value::as_object)
    else {
        return out;
    };
    for key in ["packs", "links", "no_continuations", "scenario"] {
        if let Some(v) = props.get(key) {
            out.insert(key.to_string(), v.clone());
        }
    }
    out
}

/// The `Op` variant in the `Request` schema's `$defs` whose `op` const is
/// `name`, if any.
fn op_variant(name: &str) -> Option<&'static Value> {
    request_schema()
        .get("$defs")?
        .get("Op")?
        .get("oneOf")?
        .as_array()?
        .iter()
        .find(|v| {
            v.get("properties")
                .and_then(|p| p.get("op"))
                .and_then(|o| o.get("const"))
                .and_then(Value::as_str)
                == Some(name)
        })
}

/// The MCP input schema for op `name`: the shared request fields plus that
/// op variant's own fields (minus the `op` tag), `$defs` carried over
/// unmodified so every `$ref` (e.g. to `Scenario`, `StructureWhat`) still
/// resolves. `None` for an unknown op name.
fn op_input_schema(name: &str) -> Option<JsonObject> {
    let variant = op_variant(name)?;
    let mut properties = shared_properties();
    if let Some(vp) = variant.get("properties").and_then(Value::as_object) {
        for (k, v) in vp {
            if k != "op" {
                properties.insert(k.clone(), v.clone());
            }
        }
    }
    let required: Vec<Value> = variant
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|v| v.as_str() != Some("op"))
        .cloned()
        .collect();

    let mut schema = Map::new();
    schema.insert(
        "$schema".into(),
        json!("https://json-schema.org/draft/2020-12/schema"),
    );
    schema.insert("type".into(), json!("object"));
    schema.insert("properties".into(), Value::Object(properties));
    if !required.is_empty() {
        schema.insert("required".into(), Value::Array(required));
    }
    schema.insert("additionalProperties".into(), json!(false));
    if let Some(defs) = request_schema().get("$defs") {
        schema.insert("$defs".into(), defs.clone());
    }
    Some(schema)
}

fn op_description(manual: &Value, name: &str) -> String {
    manual
        .get("ops")
        .and_then(|o| o.get(name))
        .and_then(Value::as_str)
        .map_or_else(|| format!("litgraph op `{name}`"), str::to_string)
}

fn generic_tool() -> Tool {
    let schema = request_schema().as_object().cloned().unwrap_or_default();
    Tool::new(
        GENERIC_TOOL,
        "Run any litgraph request `{packs?, links?, no_continuations?, scenario?, op}` \
         verbatim -- the escape hatch for anything not exposed as its own tool. See the \
         `litgraph://describe` resource, or the `describe` tool, for the full manual.",
        schema,
    )
}

/// Every tool this server exposes: one per [`OP_NAMES`], plus
/// [`GENERIC_TOOL`]. Descriptions are the exact text
/// `litgraph::api::describe`'s `"ops"` map uses for each op, so a tool's
/// description can't drift from the CLI's own manual either.
#[must_use]
pub fn tools(catalog: &Catalog) -> Vec<Tool> {
    let manual = api::describe(catalog);
    let mut out: Vec<Tool> = OP_NAMES
        .iter()
        .filter_map(|&name| {
            let schema = op_input_schema(name)?;
            Some(Tool::new(name, op_description(&manual, name), schema))
        })
        .collect();
    out.push(generic_tool());
    out
}

/// Build the full request envelope for op `name` from an MCP tool call's
/// flat arguments (`{packs?, links?, no_continuations?, scenario?,
/// ...op fields}`) and run it. Never panics: an unknown field, bad type,
/// or engine error all come back as `ok: false` in the returned envelope,
/// exactly as `litgraph::api::handle_json` already guarantees.
#[must_use]
pub fn call_op(catalog: &Catalog, name: &str, arguments: Option<JsonObject>) -> Response {
    let mut args = arguments.unwrap_or_default();
    let packs = args.remove("packs").unwrap_or_else(|| json!([]));
    let links = args.remove("links").unwrap_or_else(|| json!(true));
    let no_continuations = args
        .remove("no_continuations")
        .unwrap_or_else(|| json!(false));
    let scenario = args.remove("scenario").unwrap_or_else(|| json!({}));
    args.insert("op".into(), json!(name));
    let request = json!({
        "packs": packs,
        "links": links,
        "no_continuations": no_continuations,
        "scenario": scenario,
        "op": Value::Object(args),
    });
    api::handle_json(&request.to_string(), catalog)
}

/// Run a raw [`Request`](api::Request) (the [`GENERIC_TOOL`] tool's
/// arguments, taken verbatim). Never panics, for the same reason as
/// [`call_op`].
#[must_use]
pub fn call_raw(catalog: &Catalog, arguments: Option<JsonObject>) -> Response {
    let request = Value::Object(arguments.unwrap_or_default());
    api::handle_json(&request.to_string(), catalog)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> Catalog {
        Catalog::embedded().expect("embedded packs must load in tests")
    }

    #[test]
    fn op_names_match_the_schemas_oneof_variants() {
        let defs = request_schema()["$defs"]["Op"]["oneOf"].as_array().unwrap();
        let schema_names: Vec<&str> = defs
            .iter()
            .map(|v| v["properties"]["op"]["const"].as_str().unwrap())
            .collect();
        assert_eq!(schema_names, OP_NAMES);
    }

    #[test]
    fn tools_covers_every_op_plus_the_generic_one() {
        let c = catalog();
        let names: Vec<String> = tools(&c).into_iter().map(|t| t.name.to_string()).collect();
        for op in OP_NAMES {
            assert!(names.contains(&(*op).to_string()), "missing tool for {op}");
        }
        assert!(names.contains(&GENERIC_TOOL.to_string()));
        assert_eq!(names.len(), OP_NAMES.len() + 1);
    }

    #[test]
    fn op_schema_drops_the_tag_and_keeps_shared_fields() {
        let schema = op_input_schema("solve").unwrap();
        let props = schema["properties"].as_object().unwrap();
        assert!(
            !props.contains_key("op"),
            "the `op` tag must not leak into the tool schema"
        );
        for field in ["from", "full_policy", "all_values", "max_steps"] {
            assert!(props.contains_key(field), "missing solve field {field}");
        }
        for field in ["packs", "links", "no_continuations", "scenario"] {
            assert!(props.contains_key(field), "missing shared field {field}");
        }
        assert_eq!(schema["additionalProperties"], json!(false));
        assert!(schema["$defs"].is_object());
    }

    #[test]
    fn op_schema_keeps_non_op_required_fields() {
        // `metric` has a required, no-default `spec` field.
        let schema = op_input_schema("metric").unwrap();
        let required: Vec<&str> = schema["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(required, vec!["spec"]);
    }

    #[test]
    fn op_input_schema_is_none_for_an_unknown_op() {
        assert!(op_input_schema("nope").is_none());
    }

    #[test]
    fn call_op_solves_with_flat_arguments() {
        let c = catalog();
        let mut args = Map::new();
        args.insert("packs".into(), json!(["cofc"]));
        let resp = call_op(&c, "solve", Some(args));
        assert!(resp.ok, "{resp:?}");
        assert_eq!(resp.op, "solve");
        assert!(resp.result.unwrap()["value"].is_number());
    }

    #[test]
    fn call_op_rejects_an_unknown_field_as_ok_false_not_a_panic() {
        let c = catalog();
        let mut args = Map::new();
        args.insert("bogus_field".into(), json!(true));
        let resp = call_op(&c, "solve", Some(args));
        assert!(!resp.ok);
        assert_eq!(resp.error.unwrap().code, "parse");
    }

    #[test]
    fn call_op_on_the_default_arguments_describes() {
        let c = catalog();
        let resp = call_op(&c, "describe", None);
        assert!(resp.ok, "{resp:?}");
    }

    #[test]
    fn call_raw_runs_a_full_request_verbatim() {
        let c = catalog();
        let mut args = Map::new();
        args.insert("packs".into(), json!(["cofc"]));
        args.insert("op".into(), json!({ "op": "packs" }));
        let resp = call_raw(&c, Some(args));
        assert!(resp.ok, "{resp:?}");
        assert_eq!(resp.op, "packs");
    }

    #[test]
    fn call_raw_with_no_arguments_defaults_to_describe() {
        let c = catalog();
        let resp = call_raw(&c, None);
        assert!(resp.ok);
        assert_eq!(resp.op, "describe");
    }
}
