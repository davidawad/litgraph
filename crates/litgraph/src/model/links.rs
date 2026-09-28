// SPDX-License-Identifier: GPL-3.0-or-later
//! `links.json`: cross-pack edges and pack instances.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

use super::schema::{NodeKind, Pack, RawEdge, Role};
use crate::error::{Error, Result};

/// A namespaced copy of a pack that remembers how it was entered — the
/// product-graph construction for one piece of history, expressed as data.
/// `cafc@cofc` is the Federal Circuit as entered from the Court of Federal
/// Claims: its remand router can only route back to the `CoFC`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Instance {
    /// Base pack id.
    pub pack: String,
    /// Why this instance exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Local edge ids of the base pack to drop.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove_edges: Vec<String>,
    /// Local edge id → probability.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub probabilities: BTreeMap<String, f64>,
    /// actor → role override (e.g. flip appellant/appellee perspective).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub roles: BTreeMap<String, Role>,
    /// Expression over `payoff` mapping base terminal payoffs into this
    /// instance's perspective, e.g. `1000000 - payoff`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payoff_transform: Option<String>,
    /// Local edge id → JSON merge-patch (RFC 7386) applied to that edge.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub patch_edges: BTreeMap<String, serde_json::Value>,
}

/// Contents of `packs/links.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LinkFile {
    /// Instance name (`base@origin`) → definition.
    #[serde(default)]
    pub instances: BTreeMap<String, Instance>,
    /// Cross-pack edges; `from`/`to` are qualified ids.
    #[serde(default)]
    pub links: Vec<RawEdge>,
}

/// JSON merge-patch (RFC 7386): `null` deletes, objects merge, anything else replaces.
pub fn merge_patch(base: &mut serde_json::Value, patch: &serde_json::Value) {
    match (base.as_object_mut(), patch.as_object()) {
        (Some(b), Some(p)) => {
            for (k, v) in p {
                if v.is_null() {
                    b.remove(k);
                } else {
                    merge_patch(b.entry(k.clone()).or_insert(serde_json::Value::Null), v);
                }
            }
        }
        _ => *base = patch.clone(),
    }
}

/// Local edge ids as the compiler derives them: the authored `id`, else
/// `from->to#n` where `n` counts unnamed parallel edges from 0.
#[must_use]
pub fn local_edge_ids(edges: &[RawEdge]) -> Vec<String> {
    let mut seen: HashMap<(&str, &str), usize> = HashMap::new();
    edges
        .iter()
        .map(|e| {
            if let Some(id) = &e.id {
                return id.clone();
            }
            let n = seen.entry((e.from.as_str(), e.to.as_str())).or_insert(0);
            let id = format!("{}->{}#{}", e.from, e.to, n);
            *n += 1;
            id
        })
        .collect()
}

struct PayoffEnv(f64);

impl crate::expr::Env for PayoffEnv {
    fn var(&self, name: &str) -> Option<f64> {
        (name == "payoff").then_some(self.0)
    }
    fn func(&self, _: &str, _: &[crate::expr::Arg]) -> Option<Result<f64>> {
        None
    }
}

impl Instance {
    /// Build the instance pack from its base.
    ///
    /// # Errors
    /// Unknown edge ids, an invalid patch, or a bad `payoff_transform`.
    pub fn materialize(&self, name: &str, base: &Pack) -> Result<Pack> {
        let ids = local_edge_ids(&base.edges);
        let referenced = self
            .remove_edges
            .iter()
            .chain(self.patch_edges.keys())
            .chain(self.probabilities.keys());
        for r in referenced {
            if !ids.contains(r) {
                return Err(Error::Invalid(format!("instance {name}: unknown edge {r}")));
            }
        }
        let mut edges = Vec::with_capacity(base.edges.len());
        for (e, id) in base.edges.iter().zip(&ids) {
            if self.remove_edges.contains(id) {
                continue;
            }
            let mut e = e.clone();
            e.id = Some(id.clone());
            if let Some(p) = self.probabilities.get(id) {
                e.probability = Some(*p);
            }
            if let Some(patch) = self.patch_edges.get(id) {
                let mut v = serde_json::to_value(&e).map_err(|x| Error::Invalid(x.to_string()))?;
                merge_patch(&mut v, patch);
                e = serde_json::from_value(v)
                    .map_err(|x| Error::Invalid(format!("instance {name}: patch for {id}: {x}")))?;
            }
            edges.push(e);
        }
        let mut p = base.clone();
        p.id = name.to_string();
        p.title = format!("{} [{name}]", base.title);
        p.edges = edges;
        p.roles.extend(self.roles.clone());
        if let Some(src) = &self.payoff_transform {
            let ex = crate::expr::parse(src)?;
            for n in p
                .nodes
                .iter_mut()
                .filter(|n| n.kind == Some(NodeKind::Terminal))
            {
                if let Some(x) = n.payoff {
                    n.payoff = Some(ex.eval(&PayoffEnv(x))?);
                }
            }
        }
        Ok(p)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use serde_json::json;

    #[test]
    fn merge_patch_follows_rfc7386() {
        let mut a = json!({"a": 1, "b": {"c": 2, "d": 3}});
        merge_patch(&mut a, &json!({"a": null, "b": {"c": 9}, "e": [1]}));
        assert_eq!(a, json!({"b": {"c": 9, "d": 3}, "e": [1]}));
        let mut s = json!(1);
        merge_patch(&mut s, &json!({"x": 1}));
        assert_eq!(s, json!({"x": 1}));
    }

    #[test]
    fn local_ids_count_only_unnamed_parallels() {
        let e = |id: Option<&str>| RawEdge {
            id: id.map(String::from),
            from: "a".into(),
            to: "b".into(),
            ..Default::default()
        };
        let ids = local_edge_ids(&[e(Some("x")), e(None), e(None)]);
        assert_eq!(ids, ["x", "a->b#0", "a->b#1"]);
    }

    fn base_pack() -> Pack {
        Pack::from_json(
            r#"{
                "schemaVersion": 2, "id": "base", "title": "Base", "startNodeId": "start",
                "nodes": [
                    {"id": "start", "label": "Start"},
                    {"id": "win", "label": "Win", "kind": "terminal", "payoff": 500000.0},
                    {"id": "unpriced", "label": "Unpriced", "kind": "terminal"}
                ],
                "edges": [
                    {"from": "start", "to": "win", "label": "go"}
                ]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn materialize_runs_payoff_transform_through_a_function_call() {
        // A function call in `payoff_transform` routes through PayoffEnv::func
        // (which only ever knows the bare `payoff` variable) before falling
        // back to the expression language's own math functions.
        let base = base_pack();
        let inst = Instance {
            pack: "base".into(),
            payoff_transform: Some("min(payoff, 100000)".into()),
            ..Instance::default()
        };
        let p = inst.materialize("base@x", &base).unwrap();
        assert_eq!(p.nodes[1].payoff, Some(100_000.0));
        // A terminal with no authored payoff is left alone (no crash on `None`).
        assert_eq!(p.nodes[2].payoff, None);
    }

    #[test]
    fn materialize_rejects_a_reference_to_an_unknown_edge() {
        let base = base_pack();
        let inst = Instance {
            pack: "base".into(),
            remove_edges: vec!["no-such-edge".into()],
            ..Instance::default()
        };
        let err = inst.materialize("base@x", &base).unwrap_err();
        assert_eq!(err.code(), "invalid");
        assert!(err.to_string().contains("unknown edge no-such-edge"));
    }
}
