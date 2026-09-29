// SPDX-License-Identifier: GPL-3.0-or-later
//! The compiled, composed graph: the [`Graph`] container and the
//! compilation logic that builds it. See `entities.rs` for what a compiled
//! node/edge looks like ([`Node`], [`Edge`], [`PackMeta`], [`PayoffSource`],
//! [`CompileOptions`]).

use std::collections::{BTreeMap, HashMap};

use super::entities::{CompileOptions, Edge, Node, PackMeta, PayoffSource};
use super::links::{local_edge_ids, LinkFile};
use super::schema::{default_roles, NodeKind, Pack, RawEdge, Role};
use crate::error::{Error, Result};

/// Dense node index.
pub type NodeIx = usize;
/// Dense edge index.
pub type EdgeIx = usize;

/// One or more packs compiled into dense index space, ids namespaced `pack::local`.
#[derive(Debug, Clone)]
pub struct Graph {
    /// Nodes.
    pub nodes: Vec<Node>,
    /// Edges.
    pub edges: Vec<Edge>,
    /// Out-edges per node.
    pub out: Vec<Vec<EdgeIx>>,
    /// In-edges per node.
    pub inc: Vec<Vec<EdgeIx>>,
    /// Default start (the first pack's start).
    pub start: NodeIx,
    /// Compiled packs and instances.
    pub packs: Vec<PackMeta>,
    /// Per-pack actor → role tables.
    pub pack_roles: BTreeMap<String, BTreeMap<String, Role>>,
    /// Compile-time notes (continuations, superseded edges).
    pub notes: Vec<String>,
    pub(super) node_ix: HashMap<String, NodeIx>,
    pub(super) edge_ix: HashMap<String, EdgeIx>,
}

/// v1 terminal payoff heuristic (1 scoring point = $1,000). Only understands
/// patent-prosecution vocabulary; always reported as `heuristic`.
#[must_use]
pub fn heuristic_payoff(id: &str, label: &str) -> Option<f64> {
    let (id, label) = (id.to_lowercase(), label.to_lowercase());
    let has = |s: &str| id.contains(s) || label.contains(s);
    [
        ("issued", 150_000.0),
        ("abandon", -75_000.0),
        ("expired", 0.0),
        ("cancel", -100_000.0),
        ("confirm", 150_000.0),
        ("amend", 75_000.0),
    ]
    .into_iter()
    .find(|(k, _)| has(k))
    .map(|(_, v)| v)
}

/// `pack::local`, unless `local` is already qualified.
#[must_use]
pub fn qualify(pack: &str, local: &str) -> String {
    if local.contains("::") {
        local.to_string()
    } else {
        format!("{pack}::{local}")
    }
}

fn pack_of(qualified: &str) -> &str {
    qualified.split("::").next().unwrap_or("")
}

impl Graph {
    /// Compile packs, their `links.json` instances, and cross-pack links into
    /// one graph. Links apply only when both endpoints' packs are loaded.
    ///
    /// ```
    /// use litgraph::model::{CompileOptions, Graph, LinkFile, Pack};
    /// let json = r#"{
    ///     "schemaVersion": 2, "id": "demo", "title": "Demo", "startNodeId": "start",
    ///     "nodes": [
    ///         {"id": "start", "label": "Start"},
    ///         {"id": "end", "label": "End", "kind": "terminal", "payoff": 100.0}
    ///     ],
    ///     "edges": [{"from": "start", "to": "end", "label": "go"}]
    /// }"#;
    /// let pack = Pack::from_json(json).unwrap();
    /// let g = Graph::compile(&[pack], &LinkFile::default(), &CompileOptions::default()).unwrap();
    /// assert_eq!(g.nodes.len(), 2);
    /// assert_eq!(g.start, g.node("demo::start").unwrap());
    /// ```
    ///
    /// # Errors
    /// Duplicate or dangling ids, unknown start nodes, bad instances or links.
    pub fn compile(packs: &[Pack], lf: &LinkFile, opts: &CompileOptions) -> Result<Graph> {
        if packs.is_empty() {
            return Err(Error::Invalid("no packs given".into()));
        }
        let mut g = Graph {
            nodes: vec![],
            edges: vec![],
            out: vec![],
            inc: vec![],
            start: 0,
            packs: vec![],
            pack_roles: BTreeMap::new(),
            notes: vec![],
            node_ix: HashMap::new(),
            edge_ix: HashMap::new(),
        };
        for p in packs {
            g.add_pack(p, false)?;
        }
        for (name, inst) in &lf.instances {
            if let Some(base) = packs.iter().find(|p| p.id == inst.pack) {
                g.add_pack(&inst.materialize(name, base)?, true)?;
            }
        }
        g.add_links(&lf.links)?;
        if !opts.no_flags
            && g.edges.iter().any(|e| {
                !e.sets.is_empty()
                    || !e.clears.is_empty()
                    || !e.requires.is_empty()
                    || !e.forbids.is_empty()
            })
        {
            g.expand_flags(opts.max_product_nodes)?;
        }
        g.start = g.node_ix[&g.packs[0].start];
        if !opts.no_continuations {
            g.add_continuations();
        }
        g.rebuild_adjacency();
        Ok(g)
    }

    fn add_pack(&mut self, pack: &Pack, instance: bool) -> Result<()> {
        if self.packs.iter().any(|p| p.id == pack.id) {
            return Err(Error::Invalid(format!("pack {} given twice", pack.id)));
        }
        let mut roles = default_roles();
        roles.extend(pack.roles.clone());
        self.pack_roles.insert(pack.id.clone(), roles);
        for n in &pack.nodes {
            let qid = qualify(&pack.id, &n.id);
            if self.node_ix.contains_key(&qid) {
                return Err(Error::Invalid(format!("duplicate node id {qid}")));
            }
            let kind = n.kind.unwrap_or(NodeKind::State);
            let (payoff, payoff_source) = match (n.payoff, kind == NodeKind::Terminal) {
                (Some(p), _) => (p, PayoffSource::Authored),
                (None, true) => heuristic_payoff(&n.id, &n.label)
                    .map_or((0.0, PayoffSource::Default), |p| {
                        (p, PayoffSource::Heuristic)
                    }),
                (None, false) => (0.0, PayoffSource::Default),
            };
            self.node_ix.insert(qid.clone(), self.nodes.len());
            self.nodes.push(Node {
                id: qid.clone(),
                pack: pack.id.clone(),
                local_id: n.id.clone(),
                kind,
                label: n.label.clone(),
                cite: n.cite.clone(),
                note: n.note.clone(),
                group: n.group.clone(),
                valence: n.valence.clone(),
                payoff,
                payoff_source,
                outcome: n.outcome.clone(),
                tags: n.tags.clone(),
                attrs: n.attrs.clone(),
                payoff_by_flag: n.payoff_by_flag.clone(),
                synthetic: false,
                flags: vec![],
                base_id: qid,
            });
        }
        for (e, local) in pack.edges.iter().zip(local_edge_ids(&pack.edges)) {
            let (from, to) = (qualify(&pack.id, &e.from), qualify(&pack.id, &e.to));
            self.push_edge(&pack.id, &qualify(&pack.id, &local), &from, &to, e, false)?;
        }
        let start = qualify(&pack.id, &pack.start_node_id);
        if !self.node_ix.contains_key(&start) {
            return Err(Error::Invalid(format!(
                "pack {}: startNodeId {start} not found",
                pack.id
            )));
        }
        self.packs.push(PackMeta {
            id: pack.id.clone(),
            title: pack.title.clone(),
            forum: pack.forum.clone(),
            jurisdiction: pack.jurisdiction.clone(),
            schema_version: pack.schema_version,
            start,
            sources: pack.sources.clone(),
            node_count: pack.nodes.len(),
            edge_count: pack.edges.len(),
            instance,
        });
        Ok(())
    }

    fn add_links(&mut self, links: &[RawEdge]) -> Result<()> {
        let mut replaced: Vec<String> = vec![];
        for (i, l) in links.iter().enumerate() {
            let loaded = |q: &str| self.packs.iter().any(|p| p.id == pack_of(q));
            if !loaded(&l.from) || !loaded(&l.to) {
                continue;
            }
            let id = l.id.clone().unwrap_or_else(|| format!("link#{i}"));
            self.push_edge("links", &format!("links::{id}"), &l.from, &l.to, l, true)?;
            replaced.extend(l.replaces.iter().cloned());
        }
        if replaced.is_empty() {
            return Ok(());
        }
        if let Some(r) = replaced.iter().find(|r| !self.edge_ix.contains_key(*r)) {
            return Err(Error::Invalid(format!("link replaces unknown edge {r}")));
        }
        self.edges.retain(|e| !replaced.contains(&e.id));
        self.edge_ix = self
            .edges
            .iter()
            .enumerate()
            .map(|(i, e)| (e.id.clone(), i))
            .collect();
        self.notes.push(format!(
            "links superseded {} pack edge(s): {}",
            replaced.len(),
            replaced.join(", ")
        ));
        Ok(())
    }

    fn push_edge(
        &mut self,
        pack: &str,
        id: &str,
        from: &str,
        to: &str,
        e: &RawEdge,
        link: bool,
    ) -> Result<()> {
        let lookup = |n: &str, side: &str| {
            self.node_ix
                .get(n)
                .copied()
                .ok_or_else(|| Error::Invalid(format!("edge {id}: unknown {side} node {n}")))
        };
        let (f, t) = (lookup(from, "from")?, lookup(to, "to")?);
        if self.edge_ix.contains_key(id) {
            return Err(Error::Invalid(format!("duplicate edge id {id}")));
        }
        self.edge_ix.insert(id.to_string(), self.edges.len());
        self.edges.push(Edge {
            id: id.to_string(),
            base_id: id.to_string(),
            pack: pack.to_string(),
            from: f,
            to: t,
            label: e.label.clone(),
            actor: e.actor.clone(),
            authority: e.authority.clone(),
            deadline: e.deadline.clone(),
            duration: e.duration.clone(),
            cost: e.cost.unwrap_or(0.0),
            hours: e.hours.unwrap_or(0.0),
            probability: e.probability,
            valence: e.valence.clone(),
            action_id: e.action_id.clone(),
            note: e.note.clone(),
            tags: e.tags.clone(),
            attrs: e.attrs.clone(),
            link,
            synthetic: false,
            sets: e.sets.clone(),
            clears: e.clears.clone(),
            requires: e.requires.clone(),
            forbids: e.forbids.clone(),
        });
        Ok(())
    }

    fn rebuild_adjacency(&mut self) {
        self.out = vec![vec![]; self.nodes.len()];
        self.inc = vec![vec![]; self.nodes.len()];
        for (i, e) in self.edges.iter().enumerate() {
            self.out[e.from].push(i);
            self.inc[e.to].push(i);
        }
    }
}
