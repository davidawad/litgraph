// SPDX-License-Identifier: GPL-3.0-or-later
//! `deadlines` op: concrete due dates, with computation steps, for
//! authored pack `deadline` specs. Operates on the compiled [`Graph`]
//! directly (like `packs`/`lint`) — a deadline is a structural fact about
//! a forum's procedure, not a scenario-dependent quantity.

use serde_json::{json, Value};

use super::Warn;
use crate::clock::{
    self, ClockConfig, ComputedDeadline, Date, Direction, RuleSet, ServiceMethod, Unit,
};
use crate::error::{Error, Result};
use crate::model::{Deadline as PackDeadline, EdgeIx, Graph, NodeIx};

/// Arguments for [`deadlines_op`], mirroring `Op::Deadlines`'s fields.
pub(super) struct DeadlinesArgs<'a> {
    pub trigger: &'a str,
    pub node: Option<&'a str>,
    pub edge: Option<&'a str>,
    pub reachable: bool,
    pub service_method: Option<ServiceMethod>,
    pub additional_holidays: &'a [String],
    pub clerk_inaccessible: bool,
}

struct Ctx {
    trigger: Date,
    service_method: Option<ServiceMethod>,
    additional_holidays: Vec<Date>,
    clerk_inaccessible: bool,
}

fn pack_forum<'a>(g: &'a Graph, pack_id: &str) -> Option<&'a str> {
    g.packs
        .iter()
        .find(|p| p.id == pack_id)
        .and_then(|p| p.forum.as_deref())
}

/// The rule set that governs `e`'s pack. A cross-pack link edge is keyed by
/// its *origin* node's pack — no pack edge currently authors a `deadline`
/// on a `links.json` entry, so this is an untested-in-practice fallback,
/// documented rather than silently guessed.
fn ruleset_for_edge(g: &Graph, e: EdgeIx) -> Option<RuleSet> {
    let edge = &g.edges[e];
    let pack_id: &str = if edge.link {
        &g.nodes[edge.from].pack
    } else {
        &edge.pack
    };
    clock::ruleset_for_forum(pack_forum(g, pack_id), pack_id)
}

fn unit_of(d: &PackDeadline) -> Result<Unit> {
    match d.unit.as_deref() {
        None | Some("calendar") => Ok(Unit::Calendar),
        Some("court") => Ok(Unit::Court),
        Some(other) => Err(Error::Invalid(format!(
            "deadline.unit {other:?}; expected \"calendar\" or \"court\""
        ))),
    }
}

fn days_of(d: &PackDeadline) -> Result<i64> {
    if d.length < 0.0 || d.length.fract() != 0.0 || !d.length.is_finite() {
        return Err(Error::Invalid(format!(
            "deadline.length {} is not a non-negative integer",
            d.length
        )));
    }
    #[allow(clippy::cast_possible_truncation)]
    Ok(d.length as i64)
}

fn edge_result(g: &Graph, e: EdgeIx, d: &PackDeadline, computed: &ComputedDeadline) -> Value {
    let edge = &g.edges[e];
    json!({
        "edge": {
            "id": edge.id,
            "label": edge.label,
            "from": g.nodes[edge.from].id,
            "to": g.nodes[edge.to].id,
            "authority": edge.authority,
        },
        "rule_set": computed.rule_set,
        "rule_set_label": computed.rule_set.label(),
        "source_url": computed.rule_set.source_url(),
        "deadline_length": d.length,
        "unit": d.unit.clone().unwrap_or_else(|| "calendar".into()),
        "trigger": computed.anchor_date,
        "base_date": computed.base_date,
        "due_date": computed.due_date,
        "last_day_rolled": computed.last_day_rolled,
        "service_days_added": computed.service_days_added,
        "clerk_inaccessibility_applied": computed.clerk_inaccessibility_applied,
        "steps": computed.steps,
    })
}

fn compute_for_edge(g: &Graph, e: EdgeIx, ctx: &Ctx) -> Result<Value> {
    let edge = &g.edges[e];
    let d = edge
        .deadline
        .as_ref()
        .ok_or_else(|| Error::Invalid(format!("edge {} has no deadline", edge.id)))?;
    let rs = ruleset_for_edge(g, e).ok_or_else(|| {
        Error::Invalid(format!(
            "edge {}: no known FRCP/RCFC/FRAP/ITC rule set for pack {:?} (forum unrecognized)",
            edge.id, edge.pack
        ))
    })?;
    let dl = clock::DayDeadline {
        trigger: ctx.trigger,
        days: days_of(d)?,
        direction: Direction::Forward, // every authored pack deadline is "N days after" the trigger
        unit: unit_of(d)?,
        service_method: ctx.service_method,
    };
    let cfg = ClockConfig {
        additional_holidays: ctx.additional_holidays.clone(),
        clerk_inaccessible: ctx.clerk_inaccessible,
    };
    let computed = clock::compute_due_date(&dl, rs, &cfg)?;
    Ok(edge_result(g, e, d, &computed))
}

/// Every edge id reachable from `start` by following out-edges (`start`
/// itself included as the walk's root, not as an edge).
fn reachable_edges(g: &Graph, start: NodeIx) -> Vec<EdgeIx> {
    let mut seen_nodes = vec![false; g.nodes.len()];
    let mut seen_edges = vec![false; g.edges.len()];
    let mut stack = vec![start];
    seen_nodes[start] = true;
    let mut out = vec![];
    while let Some(n) = stack.pop() {
        for &e in &g.out[n] {
            if !seen_edges[e] {
                seen_edges[e] = true;
                out.push(e);
            }
            let to = g.edges[e].to;
            if !seen_nodes[to] {
                seen_nodes[to] = true;
                stack.push(to);
            }
        }
    }
    out
}

pub(super) fn deadlines_op(g: &Graph, a: &DeadlinesArgs) -> Result<(Value, Vec<Warn>)> {
    let trigger = Date::parse_iso(a.trigger)?;
    let additional_holidays: Result<Vec<Date>> = a
        .additional_holidays
        .iter()
        .map(|s| Date::parse_iso(s))
        .collect();
    let ctx = Ctx {
        trigger,
        service_method: a.service_method,
        additional_holidays: additional_holidays?,
        clerk_inaccessible: a.clerk_inaccessible,
    };

    if let Some(r) = a.edge {
        let e = g.edge(r)?;
        let v = compute_for_edge(g, e, &ctx)?;
        return Ok((
            json!({ "trigger": a.trigger, "count": 1, "deadlines": [v] }),
            vec![],
        ));
    }

    let start = match a.node {
        Some(r) => g.node(r)?,
        None => g.start,
    };
    let edges: Vec<EdgeIx> = if a.reachable {
        reachable_edges(g, start)
    } else {
        g.out[start].clone()
    };

    let mut out = vec![];
    let mut warns = vec![];
    for e in edges {
        if g.edges[e].deadline.is_none() {
            continue;
        }
        match compute_for_edge(g, e, &ctx) {
            Ok(v) => out.push(v),
            Err(err) => warns.push(Warn {
                code: "deadline-uncomputable".into(),
                at: Some(g.edges[e].id.clone()),
                message: err.to_string(),
            }),
        }
    }
    Ok((
        json!({
            "trigger": a.trigger,
            "node": g.nodes[start].id,
            "reachable": a.reachable,
            "count": out.len(),
            "deadlines": out,
        }),
        warns,
    ))
}
