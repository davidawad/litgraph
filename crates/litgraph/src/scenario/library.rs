// SPDX-License-Identifier: GPL-3.0-or-later
//! Named scenario library: matter profiles (`scenarios/*.json`) loadable by
//! name from a request's `scenario` field, alone or composed with inline
//! overrides via `extends`. See `docs/PACK_SCHEMA.md` for the shape a file
//! must have and `api::Catalog::scenarios`/`scenario` for how it is loaded.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::scenario::Scenario;

/// A primary source backing one fact or number in a named scenario (a rate,
/// a stake, a probability, a deadline). Mirrors a pack's `sources` entry.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScenarioSource {
    /// What this source backs, e.g. "bid-protest sustain rate".
    pub title: String,
    /// Official URL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Effective / retrieval date.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub as_of: Option<String>,
}

/// One named matter profile: which packs it needs, a human summary and
/// notes (facts, and the basis for every number), its sourced facts, and the
/// engine [`Scenario`] itself (rates, stakes, perspective, payoffs,
/// probabilities, policy, modeling modes).
///
/// A request references one by name (`"scenario": "<id>"`) or composes it
/// with inline overrides (`"scenario": {"extends": "<id>", ...overrides}`,
/// deep-merged per RFC 7386). See `crate::api::scenario_ref::resolve`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NamedScenario {
    /// Stable name a request references this scenario by; by convention
    /// matches the file stem (`scenarios/<id>.json`), though lookup also
    /// accepts the file stem when it differs.
    pub id: String,
    /// One-line summary; `describe` lists every scenario by this.
    pub summary: String,
    /// Free-text notes: facts, the basis for every rate/stake/probability
    /// number (sourced vs illustrative), and any modeling judgment calls.
    #[serde(default)]
    pub notes: String,
    /// Pack refs this scenario needs. A request that names this scenario and
    /// supplies no `packs` of its own uses these; an explicit `packs` on the
    /// request always wins.
    #[serde(default)]
    pub packs: Vec<String>,
    /// Sourced facts backing this scenario's numbers.
    #[serde(default)]
    pub sources: Vec<ScenarioSource>,
    /// The engine scenario: params, perspective, masks, payoffs,
    /// probabilities, policy, modeling modes.
    #[serde(default)]
    pub scenario: Scenario,
}

impl NamedScenario {
    /// Parse and sanity-check a scenario-library file.
    ///
    /// # Errors
    /// `Error::Parse` on malformed JSON, an unknown field, or an empty
    /// `id`/`summary`.
    pub fn from_json(text: &str) -> Result<NamedScenario> {
        let def: NamedScenario =
            serde_json::from_str(text).map_err(|e| Error::Parse(e.to_string()))?;
        if def.id.trim().is_empty() {
            return Err(Error::Parse("scenario: `id` must not be empty".into()));
        }
        if def.summary.trim().is_empty() {
            return Err(Error::Parse(format!(
                "scenario {}: `summary` must not be empty",
                def.id
            )));
        }
        Ok(def)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_minimal_named_scenario() -> Result<()> {
        let text = r#"{"id":"t","summary":"a test scenario","packs":["cofc"],"scenario":{"params":{"rate":900}}}"#;
        let def = NamedScenario::from_json(text)?;
        assert_eq!(def.id, "t");
        assert_eq!(def.packs, vec!["cofc".to_string()]);
        assert_eq!(def.scenario.params.get("rate"), Some(&900.0));
        Ok(())
    }

    #[test]
    fn rejects_unknown_fields() {
        let text = r#"{"id":"t","summary":"s","bogus":1}"#;
        assert!(NamedScenario::from_json(text).is_err());
    }

    #[test]
    fn rejects_empty_id_or_summary() {
        assert!(NamedScenario::from_json(r#"{"id":"","summary":"s"}"#).is_err());
        assert!(NamedScenario::from_json(r#"{"id":"t","summary":""}"#).is_err());
    }
}
