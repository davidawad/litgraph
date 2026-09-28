// SPDX-License-Identifier: GPL-3.0-or-later
//! Id resolution and small graph queries. Every lookup accepts what an agent
//! is likely to type (qualified or unique local ids, labels) and fails with
//! the candidates it could have meant.

use super::graph::{EdgeIx, Graph, NodeIx};
use super::schema::{default_roles, Role};
use crate::error::{Error, Result};

fn join<'a>(it: impl Iterator<Item = &'a str>) -> String {
    it.collect::<Vec<_>>().join(", ")
}

impl Graph {
    /// Resolve a node: qualified id, or a local id unique across loaded packs.
    ///
    /// # Errors
    /// `NotFound` (with near matches) or `Invalid` when ambiguous.
    pub fn node(&self, r: &str) -> Result<NodeIx> {
        if let Some(&ix) = self.node_ix.get(r) {
            return Ok(ix);
        }
        let hits: Vec<NodeIx> = (0..self.nodes.len())
            .filter(|&i| self.nodes[i].local_id == r)
            .collect();
        match hits.as_slice() {
            [one] => Ok(*one),
            [] => Err(Error::NotFound(format!("node {r}{}", self.suggest_node(r)))),
            many => Err(Error::Invalid(format!(
                "node {r} is ambiguous: {}",
                join(many.iter().map(|&i| self.nodes[i].id.as_str()))
            ))),
        }
    }

    /// Resolve an edge: qualified id, or a local id (`id`, `from->to#n`)
    /// unique across loaded packs.
    ///
    /// # Errors
    /// `NotFound` or `Invalid` when ambiguous.
    pub fn edge(&self, r: &str) -> Result<EdgeIx> {
        if let Some(&ix) = self.edge_ix.get(r) {
            return Ok(ix);
        }
        let suffix = format!("::{r}");
        let hits: Vec<EdgeIx> = (0..self.edges.len())
            .filter(|&i| self.edges[i].id.ends_with(&suffix))
            .collect();
        match hits.as_slice() {
            [one] => Ok(*one),
            [] => Err(Error::NotFound(format!("edge {r}"))),
            many => Err(Error::Invalid(format!(
                "edge {r} is ambiguous: {}",
                join(many.iter().map(|&i| self.edges[i].id.as_str()))
            ))),
        }
    }

    /// An out-edge of `node` by id or exact label.
    ///
    /// # Errors
    /// `NotFound` listing the node's out-edges.
    pub fn edge_at(&self, node: NodeIx, r: &str) -> Result<EdgeIx> {
        if let Ok(e) = self.edge(r) {
            if self.edges[e].from == node {
                return Ok(e);
            }
        }
        let hits: Vec<EdgeIx> = self.out[node]
            .iter()
            .copied()
            .filter(|&e| self.edges[e].label == r)
            .collect();
        match hits.as_slice() {
            [one] => Ok(*one),
            _ => Err(Error::NotFound(format!(
                "no unique out-edge '{r}' at {}; options: {}",
                self.nodes[node].id,
                join(self.out[node].iter().map(|&e| self.edges[e].id.as_str()))
            ))),
        }
    }

    fn suggest_node(&self, r: &str) -> String {
        let r = r.to_lowercase();
        let near = join(
            self.nodes
                .iter()
                .filter(|n| {
                    n.id.to_lowercase().contains(&r) || r.contains(&n.local_id.to_lowercase())
                })
                .take(5)
                .map(|n| n.id.as_str()),
        );
        if near.is_empty() {
            String::new()
        } else {
            format!(" (did you mean: {near})")
        }
    }

    /// Terminal nodes.
    pub fn terminals(&self) -> impl Iterator<Item = NodeIx> + '_ {
        (0..self.nodes.len()).filter(|&i| self.nodes[i].is_terminal())
    }

    /// Role of an edge from its pack's role table (links use the source
    /// node's pack). Scenario perspective overrides are applied by `View`.
    #[must_use]
    pub fn base_role(&self, e: EdgeIx) -> Role {
        let edge = &self.edges[e];
        let pack = if edge.link {
            &self.nodes[edge.from].pack
        } else {
            &edge.pack
        };
        self.pack_roles
            .get(pack)
            .and_then(|r| r.get(&edge.actor).copied())
            .or_else(|| default_roles().get(&edge.actor).copied())
            .unwrap_or(Role::Nature)
    }
}
