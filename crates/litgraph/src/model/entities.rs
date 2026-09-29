// SPDX-License-Identifier: GPL-3.0-or-later
//! Compiled-graph data types: [`Node`], [`Edge`], [`PackMeta`],
//! [`PayoffSource`], and the [`CompileOptions`] compilation switches.
//! `graph.rs` holds the [`super::Graph`] container and the compilation
//! logic that produces these; this module is just what a compiled node/edge
//! looks like.

use serde::Serialize;
use std::collections::BTreeMap;

use super::graph::NodeIx;
use super::schema::{Deadline, Duration, NodeKind, Source};

/// Where a node's payoff came from.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PayoffSource {
    /// `payoff` authored on the node.
    Authored,
    /// Guessed from the label by the v1 patent-vocabulary heuristic.
    Heuristic,
    /// Not a terminal, or nothing to go on: 0.
    Default,
}

/// A compiled node.
#[derive(Debug, Clone, Serialize)]
pub struct Node {
    /// Qualified id `pack::local`.
    pub id: String,
    /// Owning pack (or instance) id.
    pub pack: String,
    /// Id within the pack.
    pub local_id: String,
    /// Structural kind after composition.
    pub kind: NodeKind,
    /// Human label.
    pub label: String,
    /// Primary citation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cite: Option<String>,
    /// Commentary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Group id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Valence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valence: Option<String>,
    /// Terminal payoff in USD (0 off-terminal).
    pub payoff: f64,
    /// Provenance of `payoff`.
    pub payoff_source: PayoffSource,
    /// Outcome tags.
    pub outcome: Vec<String>,
    /// Node tags.
    pub tags: Vec<String>,
    /// Numeric attributes.
    pub attrs: BTreeMap<String, f64>,
    /// Terminal payoff override keyed by flag name (authoring data, carried
    /// through so the product-graph compiler can resolve it per copy).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub payoff_by_flag: BTreeMap<String, f64>,
    /// Engine-created (the `#end` twin of a continued terminal).
    pub synthetic: bool,
    /// State flags set at this compiled copy (empty for the base graph, or
    /// for a pack that never uses flags). Sorted.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
    /// The unflagged (`flags` empty) qualified id this node is a copy of.
    /// Equal to `id` when `flags` is empty. Lets output project a flagged
    /// product-graph node back to the base pack node an author wrote.
    pub base_id: String,
}

impl Node {
    /// True if the node carries `tag` as an outcome tag or a node tag.
    #[must_use]
    pub fn has_tag(&self, tag: &str) -> bool {
        self.outcome.iter().chain(&self.tags).any(|t| t == tag)
    }

    /// True for terminal nodes.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        self.kind == NodeKind::Terminal
    }

    /// True if this compiled node carries `flag`.
    #[must_use]
    pub fn has_flag(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }
}

/// A compiled edge.
#[derive(Debug, Clone, Serialize)]
pub struct Edge {
    /// Qualified id.
    pub id: String,
    /// Owning pack (`links` for cross-pack links).
    pub pack: String,
    /// Source node.
    pub from: NodeIx,
    /// Target node.
    pub to: NodeIx,
    /// Human label.
    pub label: String,
    /// Raw actor (read through roles).
    pub actor: String,
    /// Authority.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authority: Option<String>,
    /// Deadline window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline: Option<Deadline>,
    /// Expected duration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<Duration>,
    /// Cash fees (USD).
    pub cost: f64,
    /// Attorney hours.
    pub hours: f64,
    /// Authored probability.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probability: Option<f64>,
    /// Valence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valence: Option<String>,
    /// Rulepack action id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_id: Option<String>,
    /// Commentary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Tags.
    pub tags: Vec<String>,
    /// Numeric attributes.
    pub attrs: BTreeMap<String, f64>,
    /// Cross-pack link edge.
    pub link: bool,
    /// Engine-created edge (terminal continuation `accept`).
    pub synthetic: bool,
    /// Flags this edge sets when taken.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sets: Vec<String>,
    /// Flags this edge clears when taken.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub clears: Vec<String>,
    /// Flags that must all be set for this edge to exist in the product graph.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
    /// Flags that must all be absent for this edge to exist in the product graph.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub forbids: Vec<String>,
    /// The unflagged (pre-product-expansion) qualified id this edge is a
    /// copy of. Equal to `id` when the graph has no state-flag product
    /// (or for the empty-flag instantiation of a flagged edge). Lets a
    /// consumer that wants "every copy of this edge" — e.g. calibration,
    /// which authors one value for the edge a pack describes, not one per
    /// flag-history it can be taken under — find them all.
    pub base_id: String,
}

/// Summary of a compiled pack.
#[derive(Debug, Clone, Serialize)]
pub struct PackMeta {
    /// Pack (or instance) id.
    pub id: String,
    /// Title.
    pub title: String,
    /// Forum key.
    pub forum: Option<String>,
    /// Jurisdiction.
    pub jurisdiction: Option<String>,
    /// Schema version.
    pub schema_version: u32,
    /// Qualified start node id.
    pub start: String,
    /// Primary sources.
    pub sources: Vec<Source>,
    /// Authored node count.
    pub node_count: usize,
    /// Authored edge count.
    pub edge_count: usize,
    /// True for `links.json` instances (entered through links, not their start).
    pub instance: bool,
}

/// Hard cap on compiled `(node, flag-set)` product states. Chosen generously
/// above every real pack combination; a graph that hits it gets a clear
/// error rather than an unbounded compile.
pub const DEFAULT_MAX_PRODUCT_NODES: usize = 20_000;

/// Compilation switches.
#[derive(Debug, Clone)]
pub struct CompileOptions {
    /// Keep terminals with out-edges absorbing (v1 semantics) instead of
    /// turning them into choices with an explicit `accept` edge.
    pub no_continuations: bool,
    /// Hard cap on compiled `(node, flag-set)` product states (see
    /// [`DEFAULT_MAX_PRODUCT_NODES`]). Ignored when no edge declares
    /// `sets`/`clears`/`requires`/`forbids`, or when `no_flags` is set.
    pub max_product_nodes: usize,
    /// Ignore every edge's `sets`/`clears`/`requires`/`forbids` and compile
    /// the graph exactly as if no pack declared any (v1/no-memory
    /// semantics), even when some do. For reproducing the original v1
    /// engine's behavior bit-for-bit (see `tests/parity.rs`) on a pack that
    /// has since grown flags for a later matter; ordinary use leaves this
    /// `false`.
    pub no_flags: bool,
}

impl Default for CompileOptions {
    fn default() -> Self {
        CompileOptions {
            no_continuations: false,
            max_product_nodes: DEFAULT_MAX_PRODUCT_NODES,
            no_flags: false,
        }
    }
}
