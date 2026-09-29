// SPDX-License-Identifier: GPL-3.0-or-later
//! Applying a calibration set to a compiled graph.

use serde::Serialize;

use super::model::{CalibrationCatalog, CalibrationSet, CalibrationTarget};
use crate::error::Result;
use crate::model::{Duration, Graph};

/// One entry that was actually applied (its ref resolved in the compiled graph).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    /// What was set.
    pub target: CalibrationTarget,
    /// The ref it was set on.
    #[serde(rename = "ref")]
    pub target_ref: String,
    /// Attribute name (`node-attr` only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attr: Option<String>,
    /// The value set (`edge-probability` / `node-attr`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    /// The duration set (`edge-duration`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distribution: Option<Duration>,
    /// Sample size behind the value.
    pub n: u64,
    /// Source title.
    pub source_title: String,
    /// Source URL.
    pub source_url: String,
    /// Vintage start.
    pub vintage_start: String,
    /// Vintage end, if the source states a range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vintage_end: Option<String>,
}

/// Result of applying one named set against one compiled graph.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Application {
    /// The set id that was applied.
    pub set: String,
    /// Entries whose ref resolved (the referencing pack was loaded).
    pub applied: Vec<Applied>,
    /// Entries whose ref did not resolve — same convention as `links.json`:
    /// a multi-forum calibration set is expected to be used with a subset
    /// of packs, so an unresolved ref is not an error.
    pub skipped: usize,
}

/// Every edge sharing `ix`'s `base_id` (itself included): every
/// state-flag-product copy of the edge a pack actually describes. On a
/// graph with no state flags (or with `no_flags` set) this is just `[ix]`.
fn edge_family(g: &Graph, ix: usize) -> Vec<usize> {
    let base_id = &g.edges[ix].base_id;
    (0..g.edges.len())
        .filter(|&i| &g.edges[i].base_id == base_id)
        .collect()
}

/// Apply every entry in `set` whose ref resolves against `g`, mutating `g`
/// in place exactly as if the value had been authored in the pack. A
/// calibration entry authors one value for the edge a pack describes, not
/// one per flag-history it can be taken under, so a ref that resolves
/// applies to every state-flag-product copy of that edge (see
/// `docs/PACK_SCHEMA.md#state-flags`), not just the one instantiation whose
/// exact id matches.
#[must_use]
pub fn apply(g: &mut Graph, set: &CalibrationSet) -> Application {
    let mut applied = vec![];
    let mut skipped = 0usize;
    for e in &set.entries {
        let hit = match e.target {
            CalibrationTarget::EdgeProbability => match g.edge(&e.target_ref) {
                Ok(ix) => {
                    for i in edge_family(g, ix) {
                        g.edges[i].probability = e.value;
                    }
                    true
                }
                Err(_) => false,
            },
            CalibrationTarget::EdgeDuration => match g.edge(&e.target_ref) {
                Ok(ix) => {
                    for i in edge_family(g, ix) {
                        g.edges[i].duration.clone_from(&e.distribution);
                    }
                    true
                }
                Err(_) => false,
            },
            CalibrationTarget::NodeAttr => match g.node(&e.target_ref) {
                Ok(ix) => {
                    if let (Some(attr), Some(v)) = (&e.attr, e.value) {
                        g.nodes[ix].attrs.insert(attr.clone(), v);
                    }
                    true
                }
                Err(_) => false,
            },
        };
        if hit {
            applied.push(Applied {
                target: e.target,
                target_ref: e.target_ref.clone(),
                attr: e.attr.clone(),
                value: e.value,
                distribution: e.distribution.clone(),
                n: e.n,
                source_title: e.source.title.clone(),
                source_url: e.source.url.clone(),
                vintage_start: e.source.vintage_start.clone(),
                vintage_end: e.source.vintage_end.clone(),
            });
        } else {
            skipped += 1;
        }
    }
    Application {
        set: set.id.clone(),
        applied,
        skipped,
    }
}

/// Load `name` from the default calibration source (`$LITGRAPH_CALIBRATION`
/// or embedded) and apply it to `g`.
///
/// # Errors
/// The calibration source can't be loaded, or `name` doesn't exist in it.
pub fn apply_named(g: &mut Graph, name: &str) -> Result<Application> {
    let catalog = CalibrationCatalog::default_source()?;
    let set = catalog.select(name)?;
    Ok(apply(g, set))
}

/// Apply every named set in `names`, in order (a later set overwrites values
/// an earlier one set on the same ref).
///
/// # Errors
/// As [`apply_named`], for the first name that fails.
pub fn apply_all(g: &mut Graph, names: &[String]) -> Result<Vec<Application>> {
    names.iter().map(|n| apply_named(g, n)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CompileOptions, LinkFile, Pack};

    fn demo_graph() -> Graph {
        let json = r#"{
            "schemaVersion": 2, "id": "demo", "title": "Demo", "startNodeId": "start",
            "nodes": [
                {"id": "start", "label": "Start"},
                {"id": "win", "label": "Win", "kind": "terminal", "payoff": 100.0},
                {"id": "lose", "label": "Lose", "kind": "terminal", "payoff": 0.0}
            ],
            "edges": [
                {"id": "to-win", "from": "start", "to": "win", "label": "win", "actor": "examiner"},
                {"id": "to-lose", "from": "start", "to": "lose", "label": "lose", "actor": "examiner"}
            ]
        }"#;
        let pack = Pack::from_json(json).unwrap();
        Graph::compile(&[pack], &LinkFile::default(), &CompileOptions::default()).unwrap()
    }

    fn set_json() -> &'static str {
        r#"{
            "id": "demo-set", "title": "Demo calibration",
            "entries": [
                {
                    "target": "edge-probability", "ref": "demo::to-win", "value": 0.75,
                    "source": {"title": "t", "url": "https://example.com", "vintageStart": "2024-01-01"},
                    "n": 100
                },
                {
                    "target": "edge-duration", "ref": "demo::to-win",
                    "distribution": {"mode": 14.0},
                    "source": {"title": "t", "url": "https://example.com", "vintageStart": "2024-01-01"},
                    "n": 50
                },
                {
                    "target": "node-attr", "ref": "demo::win", "attr": "note_count", "value": 3.0,
                    "source": {"title": "t", "url": "https://example.com", "vintageStart": "2024-01-01"},
                    "n": 1
                }
            ]
        }"#
    }

    #[test]
    fn applies_every_target_kind_and_reports_it() {
        let mut g = demo_graph();
        let set = CalibrationSet::from_json(set_json()).unwrap();
        let app = apply(&mut g, &set);
        assert_eq!(app.applied.len(), 3);
        assert_eq!(app.skipped, 0);
        let e = g.edge("demo::to-win").unwrap();
        assert_eq!(g.edges[e].probability, Some(0.75));
        assert_eq!(g.edges[e].duration.as_ref().unwrap().mode, 14.0);
        let n = g.node("demo::win").unwrap();
        assert_eq!(g.nodes[n].attrs.get("note_count"), Some(&3.0));
    }

    #[test]
    fn unresolved_refs_are_skipped_not_errored() {
        let mut g = demo_graph();
        let set = CalibrationSet::from_json(
            r#"{"id":"x","title":"x","entries":[{
                "target":"edge-probability","ref":"nope::no-such-edge","value":0.5,
                "source":{"title":"t","url":"https://example.com","vintageStart":"2024-01-01"},"n":1
            }]}"#,
        )
        .unwrap();
        let app = apply(&mut g, &set);
        assert_eq!(app.applied.len(), 0);
        assert_eq!(app.skipped, 1);
    }

    #[test]
    fn apply_named_reports_not_found_for_unknown_set() {
        let mut g = demo_graph();
        let err = apply_named(&mut g, "definitely-not-a-real-set").unwrap_err();
        assert_eq!(err.code(), "not-found");
    }

    /// A calibration entry authors one value for the edge a pack describes,
    /// not one per flag-history it can be taken under: a calibrated PTAB
    /// FWD-outcome edge must also land on its `{ipr-estopped}` sibling,
    /// reachable via `cafc-vacates-remands -> fwd-issued` after an earlier
    /// FWD already set the flag (docs/CALIBRATION.md, docs/PACK_SCHEMA.md#state-flags).
    #[test]
    fn a_calibrated_value_reaches_every_flagged_sibling_of_the_edge() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        let pack = Pack::from_json(
            &std::fs::read_to_string(format!("{root}/packs/ptab-patent-trial-appeal-board.json"))
                .unwrap(),
        )
        .unwrap();
        let mut g = Graph::compile(&[pack], &LinkFile::default(), &CompileOptions::default())
            .expect("ptab pack compiles");

        let base_id =
            "ptab-patent-trial-appeal-board::fwd-issued->fwd-all-unpatentable#0".to_string();
        let flagged_id = format!("{base_id}{{ipr-estopped}}");
        // The flagged sibling must actually exist (via the Director-review
        // remand loop) or this test would trivially pass for the wrong
        // reason.
        let flagged_ix = g
            .edge(&flagged_id)
            .expect("fwd-issued's outcome edges are reachable a second time, flagged, via remand");
        assert_ne!(
            g.edges[flagged_ix].base_id, g.edges[flagged_ix].id,
            "the flagged copy's base_id should point back at the unflagged edge"
        );

        let set = CalibrationSet::from_json(&format!(
            r#"{{"id":"x","title":"x","entries":[{{
                "target":"edge-probability","ref":"{base_id}","value":0.9123,
                "source":{{"title":"t","url":"https://example.com","vintageStart":"2024-01-01"}},"n":1
            }}]}}"#
        ))
        .unwrap();
        let app = apply(&mut g, &set);
        assert_eq!(app.applied.len(), 1);
        assert_eq!(app.skipped, 0);

        let base_ix = g.edge(&base_id).unwrap();
        assert_eq!(g.edges[base_ix].probability, Some(0.9123));
        assert_eq!(
            g.edges[flagged_ix].probability,
            Some(0.9123),
            "the flagged sibling must carry the calibrated value too, not the authored default"
        );
    }
}
