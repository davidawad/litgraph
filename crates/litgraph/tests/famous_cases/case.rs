// SPDX-License-Identifier: GPL-3.0-or-later
//! The `tests/cases/<slug>.json` document and the walker that replays its
//! procedural history over the compiled graph.
//!
//! A case is one or more **legs**. Each leg starts at a node and takes one
//! step per real procedural event. A step names either the out-edge it
//! takes (`edge`, a qualified or local edge id) or, when unambiguous, just
//! the node it lands on (`to`). The walker only ever follows an out-edge of
//! the node it is standing on in the *compiled* graph, so state flags
//! (`requires`/`forbids` decide which copies exist) and `links.json` edges
//! are honored for free. Between two legs sits a **gap**: an event the real
//! case went through that the packs cannot express. A gap is an expected
//! failure. The test asserts the graph still has no such edge, so closing a
//! gap in a pack makes the test fail and tells you to merge the two legs.

use litgraph::api::Catalog;
use litgraph::model::{CompileOptions, EdgeIx, Graph, NodeIx};
use serde::Deserialize;

/// One famous case: packs, legs, gaps, expected outcome, deadline checks.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Case {
    /// Case name as reported (JSON key `case`).
    #[serde(rename = "case")]
    pub name: String,
    /// Official citation.
    pub citation: String,
    /// One paragraph: why this case is here and what it exercises.
    pub summary: String,
    /// Packs to compile (links always on).
    pub packs: Vec<String>,
    /// The walked history, in order.
    pub legs: Vec<Leg>,
    /// Real steps the packs lack (expected failures), one per leg boundary
    /// or after the last leg.
    #[serde(default)]
    pub gaps: Vec<Gap>,
    /// Where the last leg ends.
    pub outcome: Outcome,
    /// Rule-computed deadlines for key steps.
    #[serde(default)]
    pub deadlines: Vec<DeadlineCheck>,
    /// Steps the pack forces that the real case did not take (or took in a
    /// different shape). Not failures; recorded so a reader isn't misled.
    #[serde(default)]
    pub quirks: Vec<String>,
    /// Where every fact above comes from.
    pub sources: Vec<Source>,
}

/// A contiguous run of steps the compiled graph can express.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Leg {
    /// Node the leg starts at (qualified id; may carry a `{flag}` suffix).
    pub start: String,
    /// Why the leg starts here (what came before, summarized).
    pub context: String,
    /// The steps.
    pub steps: Vec<Step>,
}

/// One real procedural event mapped to one edge.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// Edge id (qualified, or local to the current node's pack), or
    /// `#accept` for a continued terminal's synthetic accept edge.
    #[serde(default)]
    pub edge: Option<String>,
    /// Target node (local or qualified id) when only one out-edge reaches it.
    #[serde(default)]
    pub to: Option<String>,
    /// What actually happened, in a lawyer's words.
    pub event: String,
    /// When it happened (`YYYY-MM-DD`, `YYYY-MM`, or `YYYY`), if sourced.
    #[serde(default)]
    pub date: Option<String>,
    /// `sources[].id` backing this step, if a specific one does.
    #[serde(default)]
    pub source: Option<String>,
}

/// A real procedural move the packs cannot express.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gap {
    /// What the real case did.
    pub event: String,
    /// Node (base id) the case was at.
    pub from: String,
    /// Node (base id) the case went to; `None` when the story simply runs
    /// past the edge of every pack (e.g. a Supreme Court merits decision).
    #[serde(default)]
    pub to: Option<String>,
    /// Why the packs can't do it, and what fixing it would take.
    pub why: String,
}

/// The node the history ends on.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    /// Base id of the terminal reached.
    pub node: String,
    /// Its `outcome` tags, exactly (JSON key `outcome`).
    #[serde(rename = "outcome")]
    pub tags: Vec<String>,
    /// State flags set on the compiled copy reached, exactly.
    #[serde(default)]
    pub flags: Vec<String>,
    /// What really happened in the end.
    pub real: String,
}

/// A deadline the `deadlines` op must compute by rule.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeadlineCheck {
    /// Qualified edge id (must be one of the walked steps).
    pub edge: String,
    /// Trigger date fed to the op.
    pub trigger: String,
    /// What the trigger date is.
    pub trigger_event: String,
    /// Rule set the op should pick (`frcp6`, `rcfc6`, `frap26`, `itc210`).
    pub rule_set: String,
    /// Due date computed by hand from the rule.
    pub due: String,
    /// The hand arithmetic.
    pub arithmetic: String,
    /// What the parties actually did, for contrast (not asserted).
    pub historical: String,
}

/// A source for the facts in the file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    /// Short id steps refer to.
    pub id: String,
    /// What it is.
    pub title: String,
    /// Where it lives.
    pub url: String,
}

/// One taken step, resolved.
pub struct Taken {
    /// The edge taken.
    pub edge: EdgeIx,
    /// The node it lands on.
    pub to: NodeIx,
}

/// A walked leg.
pub struct Walked {
    /// Resolved start node.
    pub start: NodeIx,
    /// Resolved steps, in order.
    pub taken: Vec<Taken>,
}

/// Read `tests/cases/<slug>.json`.
pub fn load(slug: &str) -> Case {
    let path = format!(
        "{}/../../tests/cases/{slug}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Compile the case's packs from the embedded catalog, links on.
pub fn compile(case: &Case) -> Graph {
    let catalog = Catalog::embedded().expect("embedded catalog loads");
    catalog
        .compile(&case.packs, true, &CompileOptions::default())
        .expect("case packs compile")
}

/// `true` if `edge`'s base id is `r`, or ends in `::r` (a local id).
fn edge_matches(g: &Graph, e: EdgeIx, r: &str) -> bool {
    let base = &g.edges[e].base_id;
    base == r || base.ends_with(&format!("::{r}"))
}

/// `true` if `n`'s base id is `r`, or ends in `::r` (a local id).
pub fn node_matches(g: &Graph, n: NodeIx, r: &str) -> bool {
    let base = &g.nodes[n].base_id;
    base == r || base.ends_with(&format!("::{r}"))
}

/// Take one step from `at`, or explain precisely why it can't be taken.
fn step(g: &Graph, at: NodeIx, s: &Step) -> Result<Taken, String> {
    // A continued terminal's synthetic `accept` edge leads to an `#end`
    // twin that shares the terminal's base id, so it is only ever taken
    // when a step asks for it by name (`"edge": "#accept"`).
    let accept = s.edge.as_deref() == Some("#accept");
    let hits: Vec<EdgeIx> = g.out[at]
        .iter()
        .copied()
        .filter(|&e| {
            g.edges[e].synthetic == accept
                && (accept || s.edge.as_deref().is_none_or(|r| edge_matches(g, e, r)))
                && s.to
                    .as_deref()
                    .is_none_or(|r| node_matches(g, g.edges[e].to, r))
        })
        .collect();
    match hits.as_slice() {
        [e] => Ok(Taken {
            edge: *e,
            to: g.edges[*e].to,
        }),
        [] => Err(format!(
            "no out-edge of {} matches edge={:?} to={:?}; options: {}",
            g.nodes[at].id,
            s.edge,
            s.to,
            g.out[at]
                .iter()
                .map(|&e| format!("{} -> {}", g.edges[e].base_id, g.nodes[g.edges[e].to].id))
                .collect::<Vec<_>>()
                .join(", ")
        )),
        many => Err(format!(
            "ambiguous step at {} (edge={:?} to={:?}): {}; name the edge",
            g.nodes[at].id,
            s.edge,
            s.to,
            many.iter()
                .map(|&e| g.edges[e].base_id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// Walk one leg. Panics with the step number and event on the first step
/// the graph can't take, so a failure reads as "the pack can't do X".
pub fn walk(g: &Graph, case: &Case, i: usize) -> Walked {
    let leg = &case.legs[i];
    let start = g
        .node(&leg.start)
        .unwrap_or_else(|e| panic!("{}: leg {} start: {e}", case.name, i + 1));
    let mut at = start;
    let mut taken = vec![];
    for (n, s) in leg.steps.iter().enumerate() {
        let t = step(g, at, s).unwrap_or_else(|e| {
            panic!(
                "{}: leg {} step {} ({}) is not a valid move: {e}",
                case.name,
                i + 1,
                n + 1,
                s.event
            )
        });
        at = t.to;
        taken.push(t);
    }
    Walked { start, taken }
}

/// The node a walked leg ends on.
pub fn end(w: &Walked) -> NodeIx {
    w.taken.last().map_or(w.start, |t| t.to)
}

/// `true` if some copy of `from` has an edge into some copy of `to`
/// (`to: None` = any out-edge other than a terminal's synthetic `accept`).
pub fn gap_is_closed(g: &Graph, gap: &Gap) -> bool {
    let origins: Vec<NodeIx> = (0..g.nodes.len())
        .filter(|&n| node_matches(g, n, &gap.from))
        .collect();
    assert!(!origins.is_empty(), "gap node {} not in graph", gap.from);
    origins.iter().flat_map(|&n| g.out[n].iter()).any(|&e| {
        let edge = &g.edges[e];
        match gap.to.as_deref() {
            Some(to) => node_matches(g, edge.to, to),
            None => !edge.synthetic,
        }
    })
}
