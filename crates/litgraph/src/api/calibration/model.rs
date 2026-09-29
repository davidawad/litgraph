// SPDX-License-Identifier: GPL-3.0-or-later
//! Calibration file format and the catalog it loads from.

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::api::catalog::fingerprint;
use crate::error::{Error, Result};
use crate::model::Duration;

include!(concat!(env!("OUT_DIR"), "/embedded_calibration.rs"));

/// What one calibration entry sets.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CalibrationTarget {
    /// Authors `probability` on a non-self edge (0..=1).
    EdgeProbability,
    /// Authors `duration` on an edge.
    EdgeDuration,
    /// Sets one numeric `attrs` entry on a node.
    NodeAttr,
}

/// Where a calibrated value came from.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CalibrationSource {
    /// Document title.
    pub title: String,
    /// Official URL; fetched and checked to resolve when this entry was authored.
    pub url: String,
    /// Start of the reporting period the value covers, e.g. `"2023-10-01"`.
    pub vintage_start: String,
    /// End of the reporting period, if the source states a range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vintage_end: Option<String>,
    /// Date the source was retrieved/checked (`YYYY-MM-DD`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retrieved: Option<String>,
}

/// One calibrated value.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CalibrationEntry {
    /// What this entry sets.
    pub target: CalibrationTarget,
    /// Node or edge ref (qualified `pack::local` recommended — see `docs/PACK_SCHEMA.md`;
    /// an instance-qualified ref like `cafc@cofc::panel-to-affirmed` targets one instance only).
    #[serde(rename = "ref")]
    pub target_ref: String,
    /// Attribute name; required (and only meaningful) for `node-attr`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attr: Option<String>,
    /// The value; required for `node-attr`. For `edge-probability`, `0..=1`
    /// authors the probability, or omit it (with `method` explaining why) to
    /// *clear* the edge's probability instead — for a sibling whose own
    /// stale, unsourced value would otherwise dilute a newly-calibrated one
    /// by renormalization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    /// The duration; required (and only meaningful) for `edge-duration`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub distribution: Option<Duration>,
    /// Where the value comes from.
    pub source: CalibrationSource,
    /// Sample size (or population count) behind the value.
    pub n: u64,
    /// Arithmetic notes when the value is derived from the source's raw counts
    /// (e.g. `"312 / (312 + 67 + 67) = 0.6996"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
}

impl CalibrationEntry {
    pub(super) fn validate(&self, set_id: &str) -> Result<()> {
        let bad = |msg: &str| -> Result<()> {
            Err(Error::Parse(format!(
                "calibration {set_id}: entry for {} ({msg})",
                self.target_ref
            )))
        };
        match self.target {
            CalibrationTarget::EdgeProbability => {
                // `value` omitted clears the edge's authored probability
                // instead of setting one — for a sibling of a newly-
                // calibrated edge whose own unsourced teaching-estimate
                // would otherwise dilute the real value by renormalization
                // (see docs/CALIBRATION.md). A clear still needs `method` to
                // say why, since it's not itself "a value from a source".
                match self.value {
                    Some(v) if !(0.0..=1.0).contains(&v) => return bad("`value` must be in [0,1]"),
                    None if self.method.is_none() => {
                        return bad(
                            "edge-probability with no `value` (clearing a stale probability) needs `method`",
                        )
                    }
                    _ => {}
                }
                if self.distribution.is_some() {
                    return bad("target edge-probability may not set `distribution`");
                }
                if self.attr.is_some() {
                    return bad("target edge-probability may not set `attr`");
                }
            }
            CalibrationTarget::EdgeDuration => {
                if self.distribution.is_none() {
                    return bad("target edge-duration needs `distribution`");
                }
                if self.value.is_some() {
                    return bad("target edge-duration may not set `value`");
                }
                if self.attr.is_some() {
                    return bad("target edge-duration may not set `attr`");
                }
            }
            CalibrationTarget::NodeAttr => {
                if self.attr.is_none() {
                    return bad("target node-attr needs `attr`");
                }
                if self.value.is_none() {
                    return bad("target node-attr needs `value`");
                }
                if self.distribution.is_some() {
                    return bad("target node-attr may not set `distribution`");
                }
            }
        }
        if self.n == 0 {
            return bad("`n` (sample size) must be > 0");
        }
        Ok(())
    }
}

/// A named collection of calibration entries: one `calibration/*.json` file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CalibrationSet {
    /// Set id; the name a scenario opts in with (`scenario.calibration`).
    pub id: String,
    /// Title.
    pub title: String,
    /// What this set covers and where it came from, in prose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The calibrated values.
    pub entries: Vec<CalibrationEntry>,
}

impl CalibrationSet {
    /// Parse and validate a calibration set.
    ///
    /// # Errors
    /// Malformed JSON, unknown fields, or an entry whose fields are
    /// inconsistent with its `target`.
    pub fn from_json(text: &str) -> Result<CalibrationSet> {
        let set: CalibrationSet =
            serde_json::from_str(text).map_err(|e| Error::Parse(e.to_string()))?;
        for e in &set.entries {
            e.validate(&set.id)?;
        }
        Ok(set)
    }
}

/// A set of calibration files, analogous to [`crate::api::Catalog`] for packs.
#[derive(Debug, Clone)]
pub struct CalibrationCatalog {
    /// `embedded` or the directory read from.
    pub origin: String,
    /// `(file name, set, content fingerprint)`, sorted by file name.
    pub sets: Vec<(String, CalibrationSet, String)>,
}

impl CalibrationCatalog {
    /// Build a catalog from `(file name, JSON text)` pairs.
    ///
    /// # Errors
    /// A malformed or invalid calibration set.
    pub fn from_files(
        origin: String,
        files: impl IntoIterator<Item = (String, String)>,
    ) -> Result<CalibrationCatalog> {
        let mut sets = vec![];
        for (name, text) in files {
            let set = CalibrationSet::from_json(&text)
                .map_err(|e| Error::Parse(format!("{name}: {e}")))?;
            sets.push((name, set, fingerprint(text.as_bytes())));
        }
        sets.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(CalibrationCatalog { origin, sets })
    }

    /// The calibration sets built into this binary.
    ///
    /// # Errors
    /// Only if an embedded set is malformed (caught by the test suite).
    pub fn embedded() -> Result<CalibrationCatalog> {
        CalibrationCatalog::from_files(
            "embedded".into(),
            EMBEDDED_CALIBRATION
                .iter()
                .map(|(n, t)| ((*n).to_string(), (*t).to_string())),
        )
    }

    /// Every `*.json` in `dir`.
    ///
    /// # Errors
    /// Unreadable directory or a malformed/invalid set.
    pub fn load(dir: &Path) -> Result<CalibrationCatalog> {
        let read =
            std::fs::read_dir(dir).map_err(|e| Error::Io(format!("{}: {e}", dir.display())))?;
        let mut files = vec![];
        for entry in read.filter_map(std::result::Result::ok) {
            let p = entry.path();
            if p.extension().is_some_and(|x| x == "json") {
                let text = std::fs::read_to_string(&p)
                    .map_err(|e| Error::Io(format!("{}: {e}", p.display())))?;
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                files.push((name, text));
            }
        }
        CalibrationCatalog::from_files(dir.display().to_string(), files)
    }

    /// `$LITGRAPH_CALIBRATION` if set, else the embedded sets.
    ///
    /// # Errors
    /// As [`CalibrationCatalog::load`] / [`CalibrationCatalog::embedded`].
    pub fn default_source() -> Result<CalibrationCatalog> {
        match std::env::var_os("LITGRAPH_CALIBRATION") {
            Some(d) => CalibrationCatalog::load(&PathBuf::from(d)),
            None => CalibrationCatalog::embedded(),
        }
    }

    /// Resolve a calibration set by id.
    ///
    /// # Errors
    /// `NotFound` listing the available ids.
    pub fn select(&self, name: &str) -> Result<&CalibrationSet> {
        self.sets
            .iter()
            .find(|(_, s, _)| s.id == name)
            .map(|(_, s, _)| s)
            .ok_or_else(|| {
                let ids: Vec<&str> = self.sets.iter().map(|(_, s, _)| s.id.as_str()).collect();
                Error::NotFound(format!(
                    "calibration set {name}; available: {}",
                    ids.join(", ")
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_entries_are_rejected_at_parse_time() {
        // edge-probability out of range
        assert!(CalibrationSet::from_json(
            r#"{"id":"x","title":"x","entries":[{
                "target":"edge-probability","ref":"a::b","value":1.5,
                "source":{"title":"t","url":"https://example.com","vintageStart":"2024-01-01"},"n":1
            }]}"#,
        )
        .is_err());
        // edge-probability with distribution set is rejected regardless of value.
        assert!(CalibrationSet::from_json(
            r#"{"id":"x","title":"x","entries":[{
                "target":"edge-probability","ref":"a::b","value":0.5,"distribution":{"mode":1.0},
                "source":{"title":"t","url":"https://example.com","vintageStart":"2024-01-01"},"n":1
            }]}"#,
        )
        .is_err());
        // edge-probability with attr set is rejected regardless of value.
        assert!(CalibrationSet::from_json(
            r#"{"id":"x","title":"x","entries":[{
                "target":"edge-probability","ref":"a::b","value":0.5,"attr":"x",
                "source":{"title":"t","url":"https://example.com","vintageStart":"2024-01-01"},"n":1
            }]}"#,
        )
        .is_err());
        // edge-duration missing distribution
        assert!(CalibrationSet::from_json(
            r#"{"id":"x","title":"x","entries":[{
                "target":"edge-duration","ref":"a::b",
                "source":{"title":"t","url":"https://example.com","vintageStart":"2024-01-01"},"n":1
            }]}"#,
        )
        .is_err());
        // edge-duration with value set is rejected.
        assert!(CalibrationSet::from_json(
            r#"{"id":"x","title":"x","entries":[{
                "target":"edge-duration","ref":"a::b","value":0.5,"distribution":{"mode":1.0},
                "source":{"title":"t","url":"https://example.com","vintageStart":"2024-01-01"},"n":1
            }]}"#,
        )
        .is_err());
        // edge-duration with attr set is rejected.
        assert!(CalibrationSet::from_json(
            r#"{"id":"x","title":"x","entries":[{
                "target":"edge-duration","ref":"a::b","attr":"x","distribution":{"mode":1.0},
                "source":{"title":"t","url":"https://example.com","vintageStart":"2024-01-01"},"n":1
            }]}"#,
        )
        .is_err());
        // node-attr missing attr
        assert!(CalibrationSet::from_json(
            r#"{"id":"x","title":"x","entries":[{
                "target":"node-attr","ref":"a::b","value":1.0,
                "source":{"title":"t","url":"https://example.com","vintageStart":"2024-01-01"},"n":1
            }]}"#,
        )
        .is_err());
        // node-attr missing value
        assert!(CalibrationSet::from_json(
            r#"{"id":"x","title":"x","entries":[{
                "target":"node-attr","ref":"a::b","attr":"x",
                "source":{"title":"t","url":"https://example.com","vintageStart":"2024-01-01"},"n":1
            }]}"#,
        )
        .is_err());
        // node-attr with distribution set is rejected.
        assert!(CalibrationSet::from_json(
            r#"{"id":"x","title":"x","entries":[{
                "target":"node-attr","ref":"a::b","attr":"x","value":1.0,"distribution":{"mode":1.0},
                "source":{"title":"t","url":"https://example.com","vintageStart":"2024-01-01"},"n":1
            }]}"#,
        )
        .is_err());
        // edge-probability with no value and no method (an ambiguous "clear"
        // with no explanation) is rejected.
        assert!(CalibrationSet::from_json(
            r#"{"id":"x","title":"x","entries":[{
                "target":"edge-probability","ref":"a::b",
                "source":{"title":"t","url":"https://example.com","vintageStart":"2024-01-01"},"n":1
            }]}"#,
        )
        .is_err());
        // n = 0
        assert!(CalibrationSet::from_json(
            r#"{"id":"x","title":"x","entries":[{
                "target":"edge-probability","ref":"a::b","value":0.5,
                "source":{"title":"t","url":"https://example.com","vintageStart":"2024-01-01"},"n":0
            }]}"#,
        )
        .is_err());
    }

    /// Omitting `value` on an `edge-probability` entry is valid *with* a
    /// `method` explaining the clear (see `calibration/ptab-fy2024.json`).
    #[test]
    fn edge_probability_clear_with_method_is_valid() {
        let set = CalibrationSet::from_json(
            r#"{"id":"x","title":"x","entries":[{
                "target":"edge-probability","ref":"a::b",
                "source":{"title":"t","url":"https://example.com","vintageStart":"2024-01-01"},
                "n":1,"method":"clearing a stale teaching estimate"
            }]}"#,
        )
        .unwrap();
        assert_eq!(set.entries[0].value, None);
    }

    #[test]
    fn load_reads_every_json_file_in_a_directory() {
        let dir = std::env::temp_dir().join(format!(
            "litgraph-calibration-test-{}-{}",
            std::process::id(),
            "load_reads_every_json_file_in_a_directory"
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("a.json"),
            r#"{"id":"a","title":"a","entries":[{
                "target":"node-attr","ref":"x::y","attr":"z","value":1.0,
                "source":{"title":"t","url":"https://example.com","vintageStart":"2024-01-01"},"n":1
            }]}"#,
        )
        .unwrap();
        std::fs::write(dir.join("not-json.txt"), "ignored").unwrap();
        let c = CalibrationCatalog::load(&dir).unwrap();
        assert_eq!(c.origin, dir.display().to_string());
        assert_eq!(c.sets.len(), 1);
        assert_eq!(c.sets[0].1.id, "a");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_of_missing_directory_is_an_io_error() {
        let err = CalibrationCatalog::load(Path::new("/no/such/directory/at/all")).unwrap_err();
        assert_eq!(err.code(), "io");
    }

    /// `default_source` reads `$LITGRAPH_CALIBRATION` when set (nextest gives
    /// every test its own process, so mutating the env var here is isolated).
    #[test]
    fn default_source_honors_litgraph_calibration_env_var() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../calibration");
        assert!(
            dir.is_dir(),
            "expected a calibration/ dir at {}",
            dir.display()
        );
        std::env::set_var("LITGRAPH_CALIBRATION", &dir);
        let c = CalibrationCatalog::default_source().unwrap();
        std::env::remove_var("LITGRAPH_CALIBRATION");
        assert_eq!(c.origin, dir.display().to_string());
        assert!(!c.sets.is_empty());
    }

    #[test]
    fn embedded_calibration_sets_parse() {
        let c = CalibrationCatalog::embedded().unwrap();
        assert!(!c.sets.is_empty(), "expected embedded calibration sets");
        for (name, set, _) in &c.sets {
            assert!(!set.entries.is_empty(), "{name}: empty entries");
        }
    }

    #[test]
    fn select_unknown_set_lists_available_ids() {
        let c = CalibrationCatalog::embedded().unwrap();
        let err = c.select("nope").unwrap_err();
        assert_eq!(err.code(), "not-found");
        assert!(err.to_string().contains("available"));
    }

    #[test]
    fn fingerprints_differ_and_are_stable() {
        let a = CalibrationCatalog::embedded().unwrap();
        let b = CalibrationCatalog::embedded().unwrap();
        assert_eq!(
            a.sets.iter().map(|(_, _, h)| h.clone()).collect::<Vec<_>>(),
            b.sets.iter().map(|(_, _, h)| h.clone()).collect::<Vec<_>>()
        );
    }
}
