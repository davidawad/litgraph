// SPDX-License-Identifier: GPL-3.0-or-later
//! Render a walked case as a plain-text story: one line per real event,
//! the pack node it lands on, and the edge (with its authority and any
//! deadline) that got it there. This is the snapshot reviewers read.

use std::fmt::Write as _;

use litgraph::model::{Graph, NodeIx, Role};

use super::case::{Case, Walked};

fn node_line(g: &Graph, n: NodeIx) -> String {
    let node = &g.nodes[n];
    let mut s = format!("{} \"{}\"", node.base_id, node.label);
    if !node.flags.is_empty() {
        let _ = write!(s, " {{{}}}", node.flags.join(","));
    }
    s
}

/// The story snapshot for `case`, walked leg by leg.
pub fn story(g: &Graph, case: &Case, walked: &[Walked], deadlines: &[String]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{}, {}", case.name, case.citation);
    let _ = writeln!(out, "{}", case.summary);
    let _ = writeln!(out, "packs: {} (links on)", case.packs.join(", "));
    for (i, (leg, w)) in case.legs.iter().zip(walked).enumerate() {
        let _ = writeln!(out, "\nleg {}: {}", i + 1, leg.context);
        let _ = writeln!(out, "  start  {}", node_line(g, w.start));
        for (n, (s, t)) in leg.steps.iter().zip(&w.taken).enumerate() {
            let e = &g.edges[t.edge];
            let date = s.date.as_deref().unwrap_or("-");
            let _ = writeln!(out, "  {:>3}. [{date}] {}", n + 1, s.event);
            let mut via = match g.base_role(t.edge) {
                Role::Me => "self",
                Role::Opponent => "opponent",
                Role::Nature => "nature",
            }
            .to_string();
            if e.link {
                via.push_str(", cross-forum link");
            }
            if let Some(a) = &e.authority {
                let _ = write!(via, ", {a}");
            }
            if let Some(d) = &e.deadline {
                let unit = d.unit.as_deref().unwrap_or("calendar");
                let _ = write!(via, ", within {} {unit} days", d.length);
            }
            let _ = writeln!(out, "       via {} ({via})", e.base_id);
            let _ = writeln!(out, "       at  {}", node_line(g, t.to));
        }
        for gap in case
            .gaps
            .iter()
            .filter(|gap| gap.from == g.nodes[super::case::end(w)].base_id)
        {
            let to = gap.to.as_deref().unwrap_or("(beyond every pack)");
            let _ = writeln!(out, "  GAP: {}", gap.event);
            let _ = writeln!(out, "       missing {} -> {to}", gap.from);
            let _ = writeln!(out, "       {}", gap.why);
        }
    }
    let _ = writeln!(out, "\noutcome: {}", case.outcome.node);
    let _ = writeln!(out, "  tags:  [{}]", case.outcome.tags.join(", "));
    if !case.outcome.flags.is_empty() {
        let _ = writeln!(out, "  flags: {{{}}}", case.outcome.flags.join(","));
    }
    let _ = writeln!(out, "  real:  {}", case.outcome.real);
    if !deadlines.is_empty() {
        let _ = writeln!(out, "\ndeadlines (by rule, from the `deadlines` op):");
        for d in deadlines {
            let _ = writeln!(out, "  {d}");
        }
    }
    if !case.quirks.is_empty() {
        let _ = writeln!(out, "\nwhere the pack's shape differs from the record:");
        for q in &case.quirks {
            let _ = writeln!(out, "  - {q}");
        }
    }
    let _ = writeln!(out, "\nsources:");
    for s in &case.sources {
        let _ = writeln!(out, "  [{}] {} <{}>", s.id, s.title, s.url);
    }
    out
}
