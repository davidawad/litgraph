// SPDX-License-Identifier: GPL-3.0-or-later
//! Packs (JSON) and the compiled, composed [`Graph`].
//!
//! A pack is one forum's procedure. A graph is one or more packs, their
//! `links.json` instances and cross-pack links, compiled into dense index
//! space with every id namespaced `pack::local`. Algorithms only ever see the
//! compiled graph; they never touch JSON.

mod graph;
mod links;
mod resolve;
mod schema;

pub use graph::{heuristic_payoff, qualify, CompileOptions, Edge, EdgeIx, Graph, Node, NodeIx, PackMeta, PayoffSource};
pub use links::{local_edge_ids, merge_patch, Instance, LinkFile};
pub use schema::{default_roles, Deadline, Duration, Group, NodeKind, Pack, RawEdge, RawNode, Role, Source};
