//! Pack files (serde) and the compiled, composed [`Graph`].
//!
//! A pack is one forum's procedure. A graph is one or more packs compiled into
//! dense index space, with every id namespaced `pack::local`. Algorithms only
//! ever see the compiled graph; they never touch JSON.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

use crate::error::{Error, Result};

pub type NodeIx = usize;
pub type EdgeIx = usize;

// ---------------------------------------------------------------------------
// Raw pack schema (v1 compatible, v2 additive). See docs/PACK_SCHEMA.md.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    State,
    Decision,
    Terminal,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Deadline {
    pub length: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extendable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extension_authority: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Duration {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    pub mode: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RawNode {
    pub id: String,
    pub kind: Option<NodeKind>,
    pub label: String,
    #[serde(default)]
    pub cite: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub group: Option<String>,
    #[serde(default)]
    pub valence: Option<String>,
    #[serde(default)]
    pub court_listener_url: Option<String>,
    #[serde(default)]
    pub payoff: Option<f64>,
    #[serde(default)]
    pub outcome: Vec<String>,
    #[serde(default)]
    pub attrs: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RawEdge {
    #[serde(default)]
    pub id: Option<String>,
    pub from: String,
    pub to: String,
    pub label: String,
    #[serde(default = "default_actor")]
    pub actor: String,
    #[serde(default)]
    pub authority: Option<String>,
    #[serde(default)]
    pub deadline: Option<Deadline>,
    #[serde(default)]
    pub duration: Option<Duration>,
    #[serde(default)]
    pub cost: Option<f64>,
    #[serde(default)]
    pub hours: Option<f64>,
    #[serde(default)]
    pub probability: Option<f64>,
    #[serde(default)]
    pub valence: Option<String>,
    #[serde(default)]
    pub court_listener_url: Option<String>,
    #[serde(default)]
    pub action_id: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub attrs: BTreeMap<String, f64>,
}

fn default_actor() -> String {
    "either".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub as_of: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Pack {
    pub schema_version: u32,
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub jurisdiction: Option<String>,
    #[serde(default)]
    pub forum: Option<String>,
    pub start_node_id: String,
    #[serde(default)]
    pub groups: Vec<serde_json::Value>,
    /// actor -> role ("self" | "opponent" | "nature").
    #[serde(default)]
    pub roles: BTreeMap<String, Role>,
    #[serde(default)]
    pub sources: Vec<Source>,
    pub nodes: Vec<RawNode>,
    pub edges: Vec<RawEdge>,
}

impl Pack {
    pub fn from_json(text: &str) -> Result<Pack> {
        let pack: Pack = serde_json::from_str(text).map_err(|e| Error::Parse(e.to_string()))?;
        if pack.schema_version != 1 && pack.schema_version != 2 {
            return Err(Error::Parse(format!(
                "pack {}: unsupported schemaVersion {}",
                pack.id, pack.schema_version
            )));
        }
        Ok(pack)
    }
}

/// A cross-pack edge. `from`/`to` are qualified ids (`pack::node`, where
/// `pack` may be an instance name like `cafc@cofc`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Link {
    #[serde(flatten)]
    pub edge: RawEdge,
    /// Qualified edge ids this link supersedes when it applies (e.g. a
    /// pack's own abstract "appeal" edge, replaced by the detailed appellate pack).
    #[serde(default)]
    pub replaces: Vec<String>,
}

/// A namespaced copy of a pack that remembers how it was entered — the
/// product-graph construction for one piece of history, expressed as data.
/// E.g. `cafc@cofc` is the Federal Circuit as entered from the Court of
/// Federal Claims: its remand router only routes back to the CoFC.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Instance {
    /// Base pack id.
    pub pack: String,
    #[serde(default)]
    pub note: Option<String>,
    /// Local edge ids of the base pack to drop in this instance.
    #[serde(default)]
    pub remove_edges: Vec<String>,
    /// Local edge id -> probability.
    #[serde(default)]
    pub probabilities: BTreeMap<String, f64>,
    /// actor -> role override (e.g. flip appellant/appellee perspective).
    #[serde(default)]
    pub roles: BTreeMap<String, Role>,
    /// Expression over `payoff` mapping base terminal payoffs into this
    /// instance's perspective, e.g. `1000000 - payoff`.
    #[serde(default)]
    pub payoff_transform: Option<String>,
    /// Local edge id -> JSON merge-patch applied to that edge (e.g. a
    /// deadline that differs when the United States is a party).
    #[serde(default)]
    pub patch_edges: BTreeMap<String, serde_json::Value>,
}

fn merge_patch(base: &mut serde_json::Value, patch: &serde_json::Value) {
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LinkFile {
    #[serde(default)]
    pub instances: BTreeMap<String, Instance>,
    #[serde(default)]
    pub links: Vec<Link>,
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
    fn materialize(&self, name: &str, base: &Pack) -> Result<Pack> {
        let mut p = base.clone();
        p.id = name.to_string();
        p.title = format!("{} [{}]", base.title, name);
        let mut k: HashMap<(String, String), usize> = HashMap::new();
        let local_ids: Vec<String> = p
            .edges
            .iter()
            .map(|e| {
                let n = k.entry((e.from.clone(), e.to.clone())).or_insert(0);
                let id =
                    e.id.clone()
                        .unwrap_or_else(|| format!("{}->{}#{}", e.from, e.to, n));
                if e.id.is_none() {
                    *n += 1;
                }
                id
            })
            .collect();
        for r in self
            .remove_edges
            .iter()
            .chain(self.patch_edges.keys())
            .chain(self.probabilities.keys())
        {
            if !local_ids.contains(r) {
                return Err(Error::Invalid(format!(
                    "instance {name}: remove_edges names unknown edge {r}"
                )));
            }
        }
        let mut edges = vec![];
        for (e, id) in p.edges.iter().zip(&local_ids) {
            if self.remove_edges.contains(id) {
                continue;
            }
            let mut e = e.clone();
            e.id = Some(id.clone());
            if let Some(pr) = self.probabilities.get(id) {
                e.probability = Some(*pr);
            }
            if let Some(patch) = self.patch_edges.get(id) {
                let mut v = serde_json::to_value(&e).map_err(|x| Error::Invalid(x.to_string()))?;
                merge_patch(&mut v, patch);
                e = serde_json::from_value(v)
                    .map_err(|x| Error::Invalid(format!("instance {name}: patch for {id}: {x}")))?;
            }
            edges.push(e);
        }
        p.edges = edges;
        p.roles.extend(self.roles.clone());
        if let Some(src) = &self.payoff_transform {
            let ex = crate::expr::parse(src)?;
            for n in &mut p.nodes {
                if n.kind == Some(NodeKind::Terminal) {
                    if let Some(x) = n.payoff {
                        n.payoff = Some(ex.eval(&PayoffEnv(x))?);
                    }
                }
            }
        }
        Ok(p)
    }
}

// ---------------------------------------------------------------------------
// Roles
// ---------------------------------------------------------------------------

/// Who controls an edge, from the analysis perspective.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// The protagonist whose policy is optimized.
    #[serde(rename = "self")]
    Me,
    /// An adversary with its own choices.
    Opponent,
    /// Tribunal / agency / chance.
    Nature,
}

/// The v1 semantics: only the applicant chooses; everything else is a draw.
pub fn default_roles() -> BTreeMap<String, Role> {
    BTreeMap::from([
        ("applicant".to_string(), Role::Me),
        ("examiner".to_string(), Role::Nature),
        ("office".to_string(), Role::Nature),
        ("either".to_string(), Role::Nature),
    ])
}

// ---------------------------------------------------------------------------
// Compiled graph
// ---------------------------------------------------------------------------

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

#[derive(Debug, Clone, Serialize)]
pub struct Node {
    pub id: String,
    pub pack: String,
    pub local_id: String,
    pub kind: NodeKind,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cite: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valence: Option<String>,
    pub payoff: f64,
    pub payoff_source: PayoffSource,
    pub outcome: Vec<String>,
    pub attrs: BTreeMap<String, f64>,
    /// Synthetic node created by composition (the `#end` twin of a continued terminal).
    pub synthetic: bool,
}

impl Node {
    pub fn is_terminal(&self) -> bool {
        self.kind == NodeKind::Terminal
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Edge {
    pub id: String,
    pub pack: String,
    pub from: NodeIx,
    pub to: NodeIx,
    pub label: String,
    pub actor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authority: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline: Option<Deadline>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<Duration>,
    pub cost: f64,
    pub hours: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probability: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub tags: Vec<String>,
    pub attrs: BTreeMap<String, f64>,
    /// Cross-pack link edge.
    pub link: bool,
    /// Engine-created edge (terminal continuation `accept`).
    pub synthetic: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PackMeta {
    pub id: String,
    pub title: String,
    pub forum: Option<String>,
    pub jurisdiction: Option<String>,
    pub schema_version: u32,
    pub start: String,
    pub sources: Vec<Source>,
    pub node_count: usize,
    pub edge_count: usize,
}

#[derive(Debug, Clone, Default)]
pub struct CompileOptions {
    /// Turn terminals that have out-edges into choice points with an
    /// explicit `accept` edge (default true). v1 treated such terminals as
    /// absorbing, which silently hid e.g. the whole post-grant region.
    pub no_continuations: bool,
}

#[derive(Debug, Clone)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub out: Vec<Vec<EdgeIx>>,
    pub inc: Vec<Vec<EdgeIx>>,
    pub start: NodeIx,
    pub packs: Vec<PackMeta>,
    /// actor -> role, merged across packs (later packs may not contradict earlier).
    pub roles: BTreeMap<String, Role>,
    /// Per-pack role overrides (pack id -> actor -> role).
    pub pack_roles: BTreeMap<String, BTreeMap<String, Role>>,
    pub notes: Vec<String>,
    node_ix: HashMap<String, NodeIx>,
    edge_ix: HashMap<String, EdgeIx>,
}

/// v1 terminal payoff heuristic (scoring.md exchange rate, 1 pt = $1,000).
/// Only understands patent-prosecution vocabulary; kept for v1 packs and
/// always reported as `heuristic` so callers know it is a guess.
pub fn heuristic_payoff(id: &str, label: &str) -> Option<f64> {
    let (id, label) = (id.to_lowercase(), label.to_lowercase());
    let has = |s: &str| id.contains(s) || label.contains(s);
    if has("issued") {
        Some(150_000.0)
    } else if has("abandon") {
        Some(-75_000.0)
    } else if has("expired") {
        Some(0.0)
    } else if has("cancel") {
        Some(-100_000.0)
    } else if has("confirm") {
        Some(150_000.0)
    } else if has("amend") {
        Some(75_000.0)
    } else {
        None
    }
}

pub fn qualify(pack: &str, local: &str) -> String {
    if local.contains("::") {
        local.to_string()
    } else {
        format!("{pack}::{local}")
    }
}

impl Graph {
    /// Compile packs (+ optional cross-pack links) into one graph. The start
    /// node is the first pack's start.
    pub fn compile(packs: &[Pack], lf: &LinkFile, opts: &CompileOptions) -> Result<Graph> {
        if packs.is_empty() {
            return Err(Error::Invalid("no packs given".into()));
        }
        // Instances of loaded packs join the compilation after the originals
        // (so the first pack's start stays the default start).
        let mut all: Vec<Pack> = packs.to_vec();
        for (name, inst) in &lf.instances {
            if let Some(base) = packs.iter().find(|p| p.id == inst.pack) {
                all.push(inst.materialize(name, base)?);
            }
        }
        let packs = &all[..];
        let links = &lf.links[..];
        let mut g = Graph {
            nodes: vec![],
            edges: vec![],
            out: vec![],
            inc: vec![],
            start: 0,
            packs: vec![],
            roles: default_roles(),
            pack_roles: BTreeMap::new(),
            notes: vec![],
            node_ix: HashMap::new(),
            edge_ix: HashMap::new(),
        };

        for pack in packs {
            if g.packs.iter().any(|p| p.id == pack.id) {
                return Err(Error::Invalid(format!("pack {} given twice", pack.id)));
            }
            let mut roles = default_roles();
            roles.extend(pack.roles.clone());
            g.pack_roles.insert(pack.id.clone(), roles);
            for n in &pack.nodes {
                let qid = qualify(&pack.id, &n.id);
                if g.node_ix.contains_key(&qid) {
                    return Err(Error::Invalid(format!("duplicate node id {qid}")));
                }
                let kind = n.kind.clone().unwrap_or(NodeKind::State);
                let (payoff, payoff_source) = match (kind == NodeKind::Terminal, n.payoff) {
                    (_, Some(p)) => (p, PayoffSource::Authored),
                    (true, None) => match heuristic_payoff(&n.id, &n.label) {
                        Some(p) => (p, PayoffSource::Heuristic),
                        None => (0.0, PayoffSource::Default),
                    },
                    (false, None) => (0.0, PayoffSource::Default),
                };
                g.node_ix.insert(qid.clone(), g.nodes.len());
                g.nodes.push(Node {
                    id: qid,
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
                    attrs: n.attrs.clone(),
                    synthetic: false,
                });
            }
            let mut parallel: HashMap<(String, String), usize> = HashMap::new();
            for e in &pack.edges {
                let key = (e.from.clone(), e.to.clone());
                let k = parallel.entry(key).or_insert(0);
                let local =
                    e.id.clone()
                        .unwrap_or_else(|| format!("{}->{}#{}", e.from, e.to, k));
                if e.id.is_none() {
                    *k += 1;
                }
                let from = qualify(&pack.id, &e.from);
                let to = qualify(&pack.id, &e.to);
                g.push_edge(&pack.id, &qualify(&pack.id, &local), &from, &to, e, false)?;
            }
            let start = qualify(&pack.id, &pack.start_node_id);
            if !g.node_ix.contains_key(&start) {
                return Err(Error::Invalid(format!(
                    "pack {}: startNodeId {start} not found",
                    pack.id
                )));
            }
            g.packs.push(PackMeta {
                id: pack.id.clone(),
                title: pack.title.clone(),
                forum: pack.forum.clone(),
                jurisdiction: pack.jurisdiction.clone(),
                schema_version: pack.schema_version,
                start,
                sources: pack.sources.clone(),
                node_count: pack.nodes.len(),
                edge_count: pack.edges.len(),
            });
        }

        let loaded: Vec<&str> = packs.iter().map(|p| p.id.as_str()).collect();
        let mut replaced: Vec<String> = vec![];
        for (i, l) in links.iter().enumerate() {
            let (from, to) = (l.edge.from.clone(), l.edge.to.clone());
            let pack_of = |q: &str| q.split("::").next().unwrap_or("").to_string();
            // Links only apply when both endpoints' packs are loaded.
            if !loaded.contains(&pack_of(&from).as_str())
                || !loaded.contains(&pack_of(&to).as_str())
            {
                continue;
            }
            let id = l.edge.id.clone().unwrap_or_else(|| format!("link#{i}"));
            g.push_edge("links", &format!("links::{id}"), &from, &to, &l.edge, true)?;
            replaced.extend(l.replaces.iter().cloned());
        }
        if !replaced.is_empty() {
            for r in &replaced {
                if !g.edge_ix.contains_key(r) {
                    return Err(Error::Invalid(format!("link replaces unknown edge {r}")));
                }
            }
            g.edges.retain(|e| !replaced.contains(&e.id));
            g.edge_ix = g
                .edges
                .iter()
                .enumerate()
                .map(|(i, e)| (e.id.clone(), i))
                .collect();
            g.notes.push(format!(
                "links superseded {} pack edge(s): {}",
                replaced.len(),
                replaced.join(", ")
            ));
        }

        g.start = g.node_ix[&g.packs[0].start];
        if !opts.no_continuations {
            g.add_continuations();
        }
        g.rebuild_adjacency();
        Ok(g)
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
        let f = *self
            .node_ix
            .get(from)
            .ok_or_else(|| Error::Invalid(format!("edge {id}: unknown from node {from}")))?;
        let t = *self
            .node_ix
            .get(to)
            .ok_or_else(|| Error::Invalid(format!("edge {id}: unknown to node {to}")))?;
        if self.edge_ix.contains_key(id) {
            return Err(Error::Invalid(format!("duplicate edge id {id}")));
        }
        self.edge_ix.insert(id.to_string(), self.edges.len());
        self.edges.push(Edge {
            id: id.to_string(),
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
        });
        Ok(())
    }

    /// Terminals with out-edges become choice/chance points with an explicit
    /// `accept` edge into an `#end` twin that carries the payoff.
    fn add_continuations(&mut self) {
        let n = self.nodes.len();
        for ix in 0..n {
            if !self.nodes[ix].is_terminal() {
                continue;
            }
            let outs: Vec<EdgeIx> = (0..self.edges.len())
                .filter(|&e| self.edges[e].from == ix)
                .collect();
            if outs.is_empty() {
                continue;
            }
            // v1 authors priced terminals as absorbing (an issued patent's value
            // is realized at issuance; maintenance → expiry is $0). Continue a
            // v1 terminal only along cross-pack links.
            let v2 = self
                .packs
                .iter()
                .any(|p| p.id == self.nodes[ix].pack && p.schema_version >= 2);
            let outs: Vec<EdgeIx> = if v2 {
                outs
            } else {
                outs.into_iter().filter(|&e| self.edges[e].link).collect()
            };
            if outs.is_empty() {
                self.notes.push(format!(
                    "v1 terminal {} has out-edges but is kept absorbing (v1 semantics); analyze the region after it with scenario.start",
                    self.nodes[ix].id
                ));
                continue;
            }
            let actors: Vec<&str> = outs.iter().map(|&e| self.edges[e].actor.as_str()).collect();
            let actor = if actors.iter().all(|a| *a == actors[0]) {
                actors[0]
            } else {
                "either"
            }
            .to_string();
            let probs: Vec<Option<f64>> = outs.iter().map(|&e| self.edges[e].probability).collect();
            let probability = if probs.iter().all(|p| p.is_some()) {
                Some((1.0 - probs.iter().map(|p| p.unwrap()).sum::<f64>()).max(0.0))
            } else {
                None
            };
            let src = self.nodes[ix].clone();
            let end_id = format!("{}#end", src.id);
            let end_ix = self.nodes.len();
            self.node_ix.insert(end_id.clone(), end_ix);
            self.nodes.push(Node {
                id: end_id,
                local_id: format!("{}#end", src.local_id),
                synthetic: true,
                ..src.clone()
            });
            let node = &mut self.nodes[ix];
            node.kind = NodeKind::Decision;
            node.payoff = 0.0;
            node.payoff_source = PayoffSource::Default;
            let edge_id = format!("{}#accept", src.id);
            self.edge_ix.insert(edge_id.clone(), self.edges.len());
            self.edges.push(Edge {
                id: edge_id,
                pack: src.pack.clone(),
                from: ix,
                to: end_ix,
                label: format!("Accept: {}", src.label),
                actor,
                authority: None,
                deadline: None,
                duration: None,
                cost: 0.0,
                hours: 0.0,
                probability,
                valence: src.valence.clone(),
                action_id: None,
                note: Some(
                    "engine-created: proceedings end here unless a continuation edge is taken"
                        .into(),
                ),
                tags: vec!["accept".into()],
                attrs: BTreeMap::new(),
                link: false,
                synthetic: true,
            });
            self.notes.push(format!(
                "terminal {} has {} out-edge(s); modeled as a choice with an explicit accept edge",
                src.id,
                outs.len()
            ));
        }
    }

    fn rebuild_adjacency(&mut self) {
        self.out = vec![vec![]; self.nodes.len()];
        self.inc = vec![vec![]; self.nodes.len()];
        for (i, e) in self.edges.iter().enumerate() {
            self.out[e.from].push(i);
            self.inc[e.to].push(i);
        }
    }

    /// Resolve a node reference: qualified id, or a local id that is unique
    /// across loaded packs.
    pub fn node(&self, r: &str) -> Result<NodeIx> {
        if let Some(&ix) = self.node_ix.get(r) {
            return Ok(ix);
        }
        let hits: Vec<NodeIx> = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.local_id == r)
            .map(|(i, _)| i)
            .collect();
        match hits.as_slice() {
            [one] => Ok(*one),
            [] => Err(Error::NotFound(format!("node {r}{}", self.suggest_node(r)))),
            many => Err(Error::Invalid(format!(
                "node {r} is ambiguous: {}",
                many.iter()
                    .map(|&i| self.nodes[i].id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        }
    }

    /// Resolve an edge reference: qualified id, local id, or `from->to` /
    /// `from->to#n` / a unique label at a given node.
    pub fn edge(&self, r: &str) -> Result<EdgeIx> {
        if let Some(&ix) = self.edge_ix.get(r) {
            return Ok(ix);
        }
        let hits: Vec<EdgeIx> = self
            .edges
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                e.id.split("::").nth(1) == Some(r) || e.id.ends_with(&format!("::{r}"))
            })
            .map(|(i, _)| i)
            .collect();
        match hits.as_slice() {
            [one] => Ok(*one),
            [] => Err(Error::NotFound(format!("edge {r}"))),
            many => Err(Error::Invalid(format!(
                "edge {r} is ambiguous: {}",
                many.iter()
                    .map(|&i| self.edges[i].id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        }
    }

    /// An out-edge of `node` by id or exact label.
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
                self.out[node]
                    .iter()
                    .map(|&e| self.edges[e].id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        }
    }

    fn suggest_node(&self, r: &str) -> String {
        let r = r.to_lowercase();
        let near: Vec<&str> = self
            .nodes
            .iter()
            .filter(|n| n.id.to_lowercase().contains(&r) || r.contains(&n.local_id.to_lowercase()))
            .take(5)
            .map(|n| n.id.as_str())
            .collect();
        if near.is_empty() {
            String::new()
        } else {
            format!(" (did you mean: {})", near.join(", "))
        }
    }

    pub fn terminals(&self) -> impl Iterator<Item = NodeIx> + '_ {
        (0..self.nodes.len()).filter(|&i| self.nodes[i].is_terminal())
    }

    /// Role table for an edge: pack roles, then the caller's perspective override.
    pub fn base_role(&self, e: EdgeIx) -> Role {
        let edge = &self.edges[e];
        let pack = if edge.link {
            self.nodes[edge.from].pack.as_str()
        } else {
            edge.pack.as_str()
        };
        self.pack_roles
            .get(pack)
            .and_then(|r| r.get(&edge.actor))
            .or_else(|| self.roles.get(&edge.actor))
            .copied()
            .unwrap_or(Role::Nature)
    }
}
