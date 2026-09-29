// SPDX-License-Identifier: GPL-3.0-or-later
//! Terminal continuations: a terminal with out-edges becomes a choice with an
//! explicit `accept` edge into an `#end` twin that keeps the payoff.

use std::collections::BTreeMap;

use super::graph::{Edge, EdgeIx, Graph, Node, NodeIx, PayoffSource};
use super::schema::NodeKind;

impl Graph {
    /// Terminals with out-edges become choice/chance points with an explicit
    /// `accept` edge into an `#end` twin that carries the payoff. v1 packs
    /// priced terminals as absorbing, so v1 terminals continue only along
    /// cross-pack links.
    pub(super) fn add_continuations(&mut self) {
        for ix in 0..self.nodes.len() {
            if !self.nodes[ix].is_terminal() {
                continue;
            }
            let v2 = self
                .packs
                .iter()
                .any(|p| p.id == self.nodes[ix].pack && p.schema_version >= 2);
            let all: Vec<EdgeIx> = (0..self.edges.len())
                .filter(|&e| self.edges[e].from == ix)
                .collect();
            let outs: Vec<EdgeIx> = all
                .iter()
                .copied()
                .filter(|&e| v2 || self.edges[e].link)
                .collect();
            if outs.is_empty() {
                if !all.is_empty() {
                    self.notes.push(format!(
                        "v1 terminal {} has out-edges but is kept absorbing (v1 semantics); analyze the region after it with scenario.start",
                        self.nodes[ix].id
                    ));
                }
                continue;
            }
            self.continue_terminal(ix, &outs);
        }
    }

    fn continue_terminal(&mut self, ix: NodeIx, outs: &[EdgeIx]) {
        let first = self.edges[outs[0]].actor.clone();
        let actor = if outs.iter().all(|&e| self.edges[e].actor == first) {
            first
        } else {
            "either".to_string()
        };
        let probability = outs
            .iter()
            .map(|&e| self.edges[e].probability)
            .sum::<Option<f64>>()
            .map(|s| (1.0 - s).max(0.0));
        let src = self.nodes[ix].clone();
        let end_ix = self.nodes.len();
        self.node_ix.insert(format!("{}#end", src.id), end_ix);
        self.nodes.push(Node {
            id: format!("{}#end", src.id),
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
                "engine-created: proceedings end here unless a continuation edge is taken".into(),
            ),
            tags: vec!["accept".into()],
            attrs: BTreeMap::new(),
            link: false,
            synthetic: true,
            sets: vec![],
            clears: vec![],
            requires: vec![],
            forbids: vec![],
        });
        self.notes.push(format!(
            "terminal {} has {} out-edge(s); modeled as a choice with an explicit accept edge",
            src.id,
            outs.len()
        ));
    }
}
