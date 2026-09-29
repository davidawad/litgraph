// SPDX-License-Identifier: GPL-3.0-or-later
//! State flags on edges (`sets`/`clears`/`requires`/`forbids`) compiled into
//! a product graph over `(node, flag-set)`.
//!
//! Litigation has memory: an IPR estoppel, a waived Rule 12(h) defense, a
//! prior RCE all change what can happen next without changing where you
//! structurally are. `docs/CRITIQUE.md` calls this the "Markov on the node"
//! limit. This module is the fix: an edge that `sets`/`clears` flags, or that
//! `requires`/`forbids` them to exist, is expanded at compile time into
//! copies of every node it can reach with a distinct flag-set, so every
//! downstream algorithm (`solve`, `chain`, `simulate`, `path`, `pareto`,
//! `sweep`, `structure`) sees a plain graph and needs no changes at all.
//!
//! Only *reachable* `(node, flag-set)` pairs are materialized (a
//! breadth-first worklist over the base edge list), and a node's
//! empty-flag copy always keeps its original id and dense index — so a pack
//! that never sets a flag compiles to an untouched graph (`Graph::compile`
//! only calls [`Graph::expand_flags`] when some edge actually carries one of
//! `sets`/`clears`/`requires`/`forbids`). See `docs/PACK_SCHEMA.md#state-flags`.

use std::collections::{BTreeSet, HashMap, VecDeque};

use super::graph::{Edge, Graph, Node, NodeIx, PayoffSource};
use crate::error::{Error, Result};

/// `{a,b}` (flags sorted, comma-joined); empty string when `flags` is empty.
#[must_use]
pub(super) fn flag_suffix(flags: &BTreeSet<String>) -> String {
    if flags.is_empty() {
        String::new()
    } else {
        let joined = flags.iter().cloned().collect::<Vec<_>>().join(",");
        format!("{{{joined}}}")
    }
}

impl Graph {
    /// Expand `sets`/`clears`/`requires`/`forbids` into a product graph.
    ///
    /// A worklist walks `(node, flag-set)` states starting from every base
    /// node with the empty flag-set (so any node remains a valid entry point
    /// — `scenario.start` can still name it directly — and the empty-flag
    /// copies are exactly the original nodes, same id, same index). Taking an
    /// edge whose `requires` aren't all set, or whose `forbids` are not all
    /// absent, is impossible in that state and the edge is simply omitted
    /// there. Otherwise the target state is `(to, (flags ∪ sets) \ clears)`;
    /// the first time a given target state is reached it is materialized as
    /// a new node `base{flag1,flag2}` (flags sorted) and queued.
    ///
    /// A terminal's `payoffByFlag` is resolved against the *target* state's
    /// flags when a new flagged copy of that terminal is created: the first
    /// matching key (sorted) overrides `payoff`, reported as authored.
    ///
    /// # Errors
    /// `Invalid` if the number of compiled nodes would exceed `cap`.
    pub(super) fn expand_flags(&mut self, cap: usize) -> Result<()> {
        let base_n = self.nodes.len();
        let base_edges = std::mem::take(&mut self.edges);
        let mut out_by_node: Vec<Vec<usize>> = vec![vec![]; base_n];
        for (i, e) in base_edges.iter().enumerate() {
            out_by_node[e.from].push(i);
        }

        // Seed every base node as its own empty-flag copy: same id, same
        // index. This is what makes the transform a no-op in effect when no
        // edge ever changes the flag-set (the guard in `compile` skips this
        // function entirely in that case, but the seeding is also correct on
        // its own).
        let mut state_ix: HashMap<(NodeIx, BTreeSet<String>), NodeIx> = HashMap::new();
        let mut nodes: Vec<Node> = Vec::with_capacity(base_n);
        let mut node_ix: HashMap<String, NodeIx> = HashMap::new();
        for (i, n) in self.nodes.iter().enumerate() {
            state_ix.insert((i, BTreeSet::new()), i);
            node_ix.insert(n.id.clone(), i);
            nodes.push(n.clone());
        }

        let mut queue: VecDeque<(NodeIx, BTreeSet<String>)> =
            (0..base_n).map(|i| (i, BTreeSet::new())).collect();
        let mut edges: Vec<Edge> = Vec::new();
        let mut edge_ix: HashMap<String, usize> = HashMap::new();

        while let Some((n, flags)) = queue.pop_front() {
            let from_ix = state_ix[&(n, flags.clone())];
            for &ei in &out_by_node[n] {
                let e = &base_edges[ei];
                if e.requires.iter().any(|r| !flags.contains(r)) {
                    continue; // a required flag isn't set here: impossible.
                }
                if e.forbids.iter().any(|f| flags.contains(f)) {
                    continue; // a forbidden flag IS set here: impossible.
                }
                let mut nf = flags.clone();
                for s in &e.sets {
                    nf.insert(s.clone());
                }
                for c in &e.clears {
                    nf.remove(c);
                }
                let to_ix = self.state_node(
                    &mut state_ix,
                    &mut nodes,
                    &mut node_ix,
                    &mut queue,
                    e.to,
                    nf,
                    cap,
                )?;
                let id = qualify_edge_id(&e.id, &flags);
                edge_ix.insert(id.clone(), edges.len());
                let mut ne = e.clone();
                ne.id = id;
                ne.from = from_ix;
                ne.to = to_ix;
                edges.push(ne);
            }
        }

        self.nodes = nodes;
        self.edges = edges;
        self.node_ix = node_ix;
        self.edge_ix = edge_ix;
        if self.nodes.len() > base_n {
            self.notes.push(format!(
                "state flags expanded the graph: {} additional node(s) for reachable flag combinations",
                self.nodes.len() - base_n
            ));
        }
        Ok(())
    }

    /// The (possibly new) node for `(base, flags)`: reuses an existing
    /// product node, or materializes and queues one, subject to `cap`.
    #[allow(clippy::too_many_arguments)]
    fn state_node(
        &self,
        state_ix: &mut HashMap<(NodeIx, BTreeSet<String>), NodeIx>,
        nodes: &mut Vec<Node>,
        node_ix: &mut HashMap<String, NodeIx>,
        queue: &mut VecDeque<(NodeIx, BTreeSet<String>)>,
        base: NodeIx,
        flags: BTreeSet<String>,
        cap: usize,
    ) -> Result<NodeIx> {
        if let Some(&ix) = state_ix.get(&(base, flags.clone())) {
            return Ok(ix);
        }
        if nodes.len() >= cap {
            return Err(Error::Invalid(format!(
                "state-flag product exceeds max_product_nodes ({cap}) at {}{}: reachable (node, flag-set) combinations exceed the cap. Clear flags more aggressively, use fewer distinct flags, or raise max_product_nodes",
                self.nodes[base].id,
                flag_suffix(&flags),
            )));
        }
        let src = &self.nodes[base];
        let suffix = flag_suffix(&flags);
        let mut nn = src.clone();
        nn.id = format!("{}{suffix}", src.id);
        nn.local_id = format!("{}{suffix}", src.local_id);
        nn.base_id.clone_from(&src.id);
        nn.flags = flags.iter().cloned().collect();
        if nn.is_terminal() {
            if let Some((_, v)) = nn.payoff_by_flag.iter().find(|(k, _)| flags.contains(*k)) {
                nn.payoff = *v;
                nn.payoff_source = PayoffSource::Authored;
            }
        }
        let ix = nodes.len();
        node_ix.insert(nn.id.clone(), ix);
        nodes.push(nn);
        state_ix.insert((base, flags.clone()), ix);
        queue.push_back((base, flags));
        Ok(ix)
    }
}

/// A product-graph edge id: the base edge id, suffixed with the flag-set it
/// was taken under (empty when `flags` is empty, so an unflagged compile
/// keeps its original edge ids verbatim).
fn qualify_edge_id(base: &str, flags: &BTreeSet<String>) -> String {
    let suffix = flag_suffix(flags);
    if suffix.is_empty() {
        base.to_string()
    } else {
        format!("{base}{suffix}")
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::model::{CompileOptions, LinkFile, Pack};

    fn compile(json: &str, cap: usize) -> Result<Graph> {
        let pack = Pack::from_json(json).unwrap();
        Graph::compile(
            &[pack],
            &LinkFile::default(),
            &CompileOptions {
                max_product_nodes: cap,
                ..CompileOptions::default()
            },
        )
    }

    const ESTOPPEL_PACK: &str = r#"{
        "schemaVersion": 2, "id": "demo", "title": "Demo", "startNodeId": "start",
        "nodes": [
            {"id": "start", "label": "Start"},
            {"id": "fwd", "label": "FWD"},
            {"id": "later", "label": "Later invalidity ground"},
            {"id": "win", "label": "Win", "kind": "terminal", "payoff": 10.0},
            {"id": "denied", "label": "Denied", "kind": "terminal", "payoff": 0.0,
             "payoffByFlag": {"estopped": -5.0, "settled": 3.0}}
        ],
        "edges": [
            {"id": "settle", "from": "start", "to": "denied", "label": "settle before FWD"},
            {"id": "go-fwd", "from": "start", "to": "fwd", "label": "reach FWD", "sets": ["estopped"]},
            {"id": "fwd-to-later", "from": "fwd", "to": "later", "label": "post-FWD"},
            {"id": "raise-again", "from": "later", "to": "win", "label": "raise the ground again",
             "forbids": ["estopped"]},
            {"id": "denied-path", "from": "later", "to": "denied", "label": "estopped instead"}
        ]
    }"#;

    /// Only reachable `(node, flag-set)` combinations are materialized: this
    /// pack has exactly one flag ever set (`estopped`), so exactly the nodes
    /// downstream of the flag-setting edge get a second, flagged copy — not
    /// a full cross product. (Every base node also keeps its own
    /// empty-flag copy so it stays directly addressable as a fresh
    /// `scenario.start`, same as before flags existed — that costs no new
    /// nodes, only extra edges from those "fresh start" copies.)
    #[test]
    fn only_reachable_flag_combinations_are_materialized() {
        let g = compile(ESTOPPEL_PACK, 1000).unwrap();
        // Base copies (5) + flagged copies of fwd/later/denied (3) = 8.
        // `win` gets no flagged copy: the only edge that reaches it
        // (`raise-again`) is `forbids: ["estopped"]`, so it never
        // instantiates from the flagged copy of `later` — see the
        // dedicated test below.
        assert_eq!(
            g.nodes.len(),
            8,
            "{:?}",
            g.nodes.iter().map(|n| &n.id).collect::<Vec<_>>()
        );
        assert!(g.node("demo::fwd{estopped}").is_ok());
        assert!(g.node("demo::later{estopped}").is_ok());
        assert!(g.node("demo::denied{estopped}").is_ok());
    }

    /// Node ids of flagged copies are stable and readable: `pack::local{flag}`.
    #[test]
    fn flagged_node_ids_are_readable() {
        let g = compile(ESTOPPEL_PACK, 1000).unwrap();
        let n = g.node("demo::fwd{estopped}").unwrap();
        assert_eq!(g.nodes[n].id, "demo::fwd{estopped}");
        assert_eq!(g.nodes[n].base_id, "demo::fwd");
        assert_eq!(g.nodes[n].flags, vec!["estopped".to_string()]);
        assert!(g.nodes[n].has_flag("estopped"));
        assert!(!g.nodes[n].has_flag("settled"));
    }

    /// `forbids` makes the specific instantiation of an edge taken from an
    /// estopped state impossible to compile, and — since the real path from
    /// where the matter starts always sets `estopped` before reaching
    /// `later` — `win` is unreachable from `g.start` even though the base
    /// (never-estopped) copies of `later`/`win`/`raise-again` still exist as
    /// directly-addressable "fresh start" nodes (same as any node has always
    /// been addressable regardless of true predecessor reachability).
    #[test]
    fn forbids_makes_the_move_impossible_along_any_real_path_from_start() {
        let g = compile(ESTOPPEL_PACK, 1000).unwrap();
        assert!(
            g.edge("raise-again{estopped}").is_err(),
            "the estopped instantiation of raise-again must not compile"
        );
        let mut seen = vec![false; g.nodes.len()];
        let mut stack = vec![g.start];
        seen[g.start] = true;
        while let Some(u) = stack.pop() {
            for &e in &g.out[u] {
                let w = g.edges[e].to;
                if !seen[w] {
                    seen[w] = true;
                    stack.push(w);
                }
            }
        }
        let win = g.node("demo::win").unwrap();
        assert!(
            !seen[win],
            "win must not be reachable from the real start once IPR estoppel is modeled"
        );
    }

    /// Terminal `payoffByFlag` overrides the base `payoff` on the matching
    /// flagged copy only; the unflagged copy keeps its authored payoff, and
    /// an unrelated flag (`settled`, reachable via `settle`, never combined
    /// with `estopped` here) resolves independently.
    #[test]
    fn payoff_by_flag_overrides_only_the_matching_copy() {
        let g = compile(ESTOPPEL_PACK, 1000).unwrap();
        let base = g.node("demo::denied").unwrap();
        assert_eq!(
            g.nodes[base].payoff, 0.0,
            "unflagged copy keeps the base payoff"
        );
        let estopped = g.node("demo::denied{estopped}").unwrap();
        assert_eq!(g.nodes[estopped].payoff, -5.0);
        assert_eq!(g.nodes[estopped].payoff_source, PayoffSource::Authored);
    }

    /// A pack with no flag-bearing edge at all compiles through the ordinary
    /// path: `expand_flags` is never invoked, so a v1/no-flags pack's graph
    /// is byte-for-byte the same shape as before flags existed (parity).
    #[test]
    fn packs_without_flags_compile_unchanged() {
        let json = r#"{
            "schemaVersion": 2, "id": "plain", "title": "Plain", "startNodeId": "a",
            "nodes": [
                {"id": "a", "label": "A"},
                {"id": "b", "label": "B", "kind": "terminal", "payoff": 5.0}
            ],
            "edges": [{"from": "a", "to": "b", "label": "go"}]
        }"#;
        let g = compile(json, 1000).unwrap();
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.edges.len(), 1);
        assert_eq!(g.nodes[0].id, "plain::a");
        assert_eq!(g.nodes[1].id, "plain::b");
        assert!(g
            .nodes
            .iter()
            .all(|n| n.flags.is_empty() && n.base_id == n.id));
        assert_eq!(g.edges[0].id, "plain::a->b#0");
    }

    /// A hard cap on the product size fails with a clear, actionable error
    /// instead of compiling forever or silently truncating.
    #[test]
    fn a_low_cap_fails_compilation_with_a_clear_error() {
        let err = compile(ESTOPPEL_PACK, 6).unwrap_err();
        assert_eq!(err.code(), "invalid");
        assert!(err.to_string().contains("max_product_nodes"), "{err}");
    }

    /// `clears` lets a flag-state return exactly to an existing state
    /// (including the empty one) instead of minting a new node forever.
    #[test]
    fn clears_can_return_to_the_empty_flag_state() {
        let json = r#"{
            "schemaVersion": 2, "id": "loop", "title": "Loop", "startNodeId": "a",
            "nodes": [
                {"id": "a", "label": "A"},
                {"id": "b", "label": "B"}
            ],
            "edges": [
                {"id": "set", "from": "a", "to": "b", "label": "set", "sets": ["x"]},
                {"id": "clear", "from": "b", "to": "a", "label": "clear", "clears": ["x"]}
            ]
        }"#;
        let g = compile(json, 1000).unwrap();
        // a, b, b{x} — clearing x on b{x}->a lands back on the ORIGINAL a
        // (flags empty), not a fourth node.
        assert_eq!(
            g.nodes.len(),
            3,
            "{:?}",
            g.nodes.iter().map(|n| &n.id).collect::<Vec<_>>()
        );
        let a0 = g.node("loop::a").unwrap();
        let bx = g.node("loop::b{x}").unwrap();
        let clear_edge = g.edge("clear{x}").unwrap();
        assert_eq!(g.edges[clear_edge].from, bx);
        assert_eq!(g.edges[clear_edge].to, a0);
    }

    /// `requires` is the positive counterpart to `forbids`: an edge is only
    /// instantiated once ALL of its required flags are set, e.g. an RCE
    /// count's second tier gated on the first having already happened.
    #[test]
    fn requires_gates_an_edge_on_every_listed_flag_being_set() {
        let json = r#"{
            "schemaVersion": 2, "id": "gated", "title": "Gated", "startNodeId": "a",
            "nodes": [
                {"id": "a", "label": "A"},
                {"id": "b", "label": "B"},
                {"id": "c", "label": "C"}
            ],
            "edges": [
                {"id": "to-b", "from": "a", "to": "b", "label": "go", "sets": ["ready"]},
                {"id": "needs-ready", "from": "b", "to": "c", "label": "advance",
                 "requires": ["ready"]}
            ]
        }"#;
        let g = compile(json, 1000).unwrap();
        // The flagged copy of b has the edge (ready is set); the seeded
        // fresh-start copy of b (empty flags) does not.
        assert!(g.edge("needs-ready{ready}").is_ok());
        assert!(g.edge("needs-ready").is_err());
    }
}
