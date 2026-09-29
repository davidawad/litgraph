// SPDX-License-Identifier: GPL-3.0-or-later
//! Resolve a request's raw `scenario` field into a concrete [`Scenario`] and
//! the packs to compile, per `docs/PACK_SCHEMA.md`'s scenario-library
//! section. `scenario` is one of:
//!
//! - absent / `null` — [`Scenario::default`], no packs implied.
//! - a string `"name"` — the named scenario's `scenario`, unmodified; its
//!   `packs` apply when the request gives none of its own.
//! - `{"extends": "name", ...overrides}` — `overrides` (every other key)
//!   deep-merged (RFC 7386) onto the named scenario's `scenario`.
//! - a plain object — an inline [`Scenario`] (today's behavior, unchanged).
//!
//! Every path that ends in a concrete [`Scenario`] deserializes it for real
//! (`deny_unknown_fields`), so a typo in an inline scenario or in `extends`
//! overrides is still a clear "unknown field" error naming the valid ones —
//! never a vague "no variant matched".

use serde_json::Value;

use super::catalog::Catalog;
use crate::error::{Error, Result};
use crate::model::merge_patch;
use crate::scenario::Scenario;

/// Resolve `raw` (a request's `scenario` field) into a [`Scenario`] and the
/// named scenario's `packs`, if any (empty when `raw` names no scenario).
fn resolve_scenario(raw: &Value, catalog: &Catalog) -> Result<(Scenario, Vec<String>)> {
    match raw {
        Value::Null => Ok((Scenario::default(), vec![])),
        Value::String(name) => {
            let def = catalog.scenario(name)?;
            Ok((def.scenario.clone(), def.packs.clone()))
        }
        Value::Object(map) => match map.get("extends") {
            Some(Value::String(name)) => {
                let def = catalog.scenario(name)?;
                let mut merged = serde_json::to_value(&def.scenario)
                    .map_err(|e| Error::Invalid(e.to_string()))?;
                let mut overrides = map.clone();
                overrides.remove("extends");
                merge_patch(&mut merged, &Value::Object(overrides));
                let sc: Scenario = serde_json::from_value(merged)
                    .map_err(|e| Error::Parse(format!("scenario: {e}")))?;
                Ok((sc, def.packs.clone()))
            }
            Some(other) => Err(Error::Parse(format!(
                "scenario.extends must be a string naming a scenario, got {other}"
            ))),
            None => {
                let sc: Scenario = serde_json::from_value(Value::Object(map.clone()))
                    .map_err(|e| Error::Parse(format!("scenario: {e}")))?;
                Ok((sc, vec![]))
            }
        },
        other => Err(Error::Parse(format!(
            "scenario must be a string (a named scenario) or an object, got {other}"
        ))),
    }
}

/// Resolve a request's `scenario` field into a [`Scenario`] plus the
/// effective pack refs to compile: `req_packs` if non-empty, else the named
/// scenario's `packs` (empty means "every pack", same as an empty `req_packs`
/// today).
///
/// # Errors
/// `NotFound` for an unknown scenario name; `Parse` for a malformed
/// `scenario` shape, a non-string `extends`, or — after composing with a
/// named base — an unknown scenario field.
pub fn resolve(
    raw: &Value,
    req_packs: &[String],
    catalog: &Catalog,
) -> Result<(Scenario, Vec<String>)> {
    let (sc, scenario_packs) = resolve_scenario(raw, catalog)?;
    let packs = if req_packs.is_empty() {
        scenario_packs
    } else {
        req_packs.to_vec()
    };
    Ok((sc, packs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn catalog() -> Catalog {
        Catalog::embedded().expect("embedded catalog compiles")
    }

    #[test]
    fn null_resolves_to_default_scenario_and_no_packs() -> Result<()> {
        let (sc, packs) = resolve(&Value::Null, &[], &catalog())?;
        assert_eq!(sc, Scenario::default());
        assert!(packs.is_empty());
        Ok(())
    }

    #[test]
    fn inline_object_behaves_exactly_as_before() -> Result<()> {
        let (sc, packs) = resolve(&json!({"params": {"rate": 900.0}}), &[], &catalog())?;
        assert_eq!(sc.params.get("rate"), Some(&900.0));
        assert!(packs.is_empty());
        Ok(())
    }

    #[test]
    fn inline_object_typo_is_a_clear_unknown_field_error() {
        let e = resolve(&json!({"bogus_field": 1}), &[], &catalog()).unwrap_err();
        assert!(e.to_string().contains("bogus_field"), "{e}");
    }

    #[test]
    fn named_string_resolves_the_catalog_scenario_and_its_packs() -> Result<()> {
        let c = catalog();
        let name = c
            .scenarios()
            .next()
            .expect("at least one scenario")
            .id
            .clone();
        let (sc, packs) = resolve(&json!(name), &[], &c)?;
        let def = c.scenario(&name)?;
        assert_eq!(sc, def.scenario);
        assert_eq!(packs, def.packs);
        Ok(())
    }

    #[test]
    fn unknown_name_is_not_found_with_a_hint() {
        let e = resolve(&json!("no-such-scenario"), &[], &catalog()).unwrap_err();
        assert_eq!(e.code(), "not-found");
        assert!(e.to_string().contains("available:"), "{e}");
    }

    #[test]
    fn request_packs_win_over_the_named_scenarios_packs() -> Result<()> {
        let c = catalog();
        let name = c
            .scenarios()
            .next()
            .expect("at least one scenario")
            .id
            .clone();
        let (_, packs) = resolve(&json!(name), &["explicit-pack".into()], &c)?;
        assert_eq!(packs, vec!["explicit-pack".to_string()]);
        Ok(())
    }

    #[test]
    fn extends_deep_merges_overrides_onto_the_named_base() -> Result<()> {
        let c = catalog();
        let name = c
            .scenarios()
            .next()
            .expect("at least one scenario")
            .id
            .clone();
        let def = c.scenario(&name)?.clone();
        let (sc, packs) = resolve(
            &json!({"extends": name, "params": {"rate": 12345.0}}),
            &[],
            &c,
        )?;
        assert_eq!(sc.params.get("rate"), Some(&12345.0));
        // Every other field of the base scenario survives the merge untouched.
        assert_eq!(sc.payoffs, def.scenario.payoffs);
        assert_eq!(packs, def.packs);
        Ok(())
    }

    #[test]
    fn extends_typo_in_overrides_is_a_clear_unknown_field_error() {
        let c = catalog();
        let name = c
            .scenarios()
            .next()
            .expect("at least one scenario")
            .id
            .clone();
        let e = resolve(&json!({"extends": name, "bogus_field": 1}), &[], &c).unwrap_err();
        assert!(e.to_string().contains("bogus_field"), "{e}");
    }

    #[test]
    fn extends_unknown_name_is_not_found() {
        let e = resolve(&json!({"extends": "no-such-scenario"}), &[], &catalog()).unwrap_err();
        assert_eq!(e.code(), "not-found");
    }

    #[test]
    fn extends_must_be_a_string() {
        let e = resolve(&json!({"extends": 1}), &[], &catalog()).unwrap_err();
        assert_eq!(e.code(), "parse");
    }

    #[test]
    fn scalar_scenario_is_a_parse_error() {
        let e = resolve(&json!(1), &[], &catalog()).unwrap_err();
        assert_eq!(e.code(), "parse");
    }
}
