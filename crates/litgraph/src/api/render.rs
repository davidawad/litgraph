// SPDX-License-Identifier: GPL-3.0-or-later
//! JSON rendering helpers shared by the ops: ids plus labels, rounded numbers.

use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

use super::Warn;
use crate::algo::{chain, mdp, paths};
use crate::error::{Error, Result};
use crate::model::NodeIx;
use crate::scenario::{Control, View, WAIT};

/// Round to 6 decimals; non-finite becomes `null`.
pub(super) fn r(x: f64) -> Value {
    if x.is_finite() {
        json!((x * 1e6).round() / 1e6)
    } else {
        Value::Null
    }
}

pub(super) fn node_ref(v: &View, n: NodeIx) -> Value {
    json!({ "id": v.g.nodes[n].id, "label": v.g.nodes[n].label })
}

pub(super) fn edge_ref(v: &View, e: usize) -> Value {
    let ed = &v.g.edges[e];
    let mut m = Map::new();
    m.insert("id".into(), json!(ed.id));
    m.insert("label".into(), json!(ed.label));
    m.insert("from".into(), json!(v.g.nodes[ed.from].id));
    m.insert("to".into(), json!(v.g.nodes[ed.to].id));
    m.insert("role".into(), json!(v.role[e]));
    if let Some(a) = &ed.authority {
        m.insert("authority".into(), json!(a));
    }
    if let Some(d) = &ed.deadline {
        m.insert("deadline_days".into(), json!(d.length));
    }
    if let Some(p) = v.prob[e] {
        m.insert("p".into(), r(p));
    }
    Value::Object(m)
}

pub(super) fn path_json(v: &View, p: &paths::Path, names: &[String]) -> Value {
    let line: Vec<String> = p
        .edges
        .iter()
        .map(|&e| {
            let ed = &v.g.edges[e];
            format!("{} --[{}]--> {}", v.g.nodes[ed.from].local_id, ed.label, v.g.nodes[ed.to].local_id)
        })
        .collect();
    json!({
        "totals": names.iter().zip(&p.totals).map(|(k, x)| (k.clone(), r(*x))).collect::<Map<_, _>>(),
        "probability": r(p.probability),
        "steps": p.edges.len(),
        "line": line,
        "edges": p.edges.iter().map(|&e| v.g.edges[e].id.clone()).collect::<Vec<_>>(),
        "end": p.nodes.last().map(|&n| node_ref(v, n)),
    })
}

/// Human label for a chosen option (an edge, or WAIT).
#[must_use]
pub fn choice_label(v: &View, e: usize) -> String {
    if e == WAIT {
        "wait (let the proceeding move)".into()
    } else {
        v.g.edges[e].label.clone()
    }
}

pub(super) fn choice_id(v: &View, e: usize) -> String {
    if e == WAIT {
        "WAIT".into()
    } else {
        v.g.edges[e].id.clone()
    }
}

/// Resolve a target spec: node ref, `terminals`, or `tag:<outcome>`.
pub(super) fn targets(v: &View, spec: &str) -> Result<Vec<NodeIx>> {
    if spec == "terminals" {
        return Ok(v.g.terminals().collect());
    }
    if let Some(tag) = spec.strip_prefix("tag:") {
        let ts: Vec<NodeIx> = v.g.terminals().filter(|&t| v.g.nodes[t].outcome.iter().any(|o| o == tag)).collect();
        if ts.is_empty() {
            return Err(Error::NotFound(format!("no terminals tagged {tag}")));
        }
        return Ok(ts);
    }
    Ok(vec![v.g.node(spec)?])
}

pub(super) fn metric_list(v: &View, names: &[String]) -> Result<Vec<(String, Vec<f64>)>> {
    names.iter().map(|n| Ok((n.clone(), v.metric(n)?))).collect()
}

/// Warnings for cyclic states where value iteration hit its cap.
pub(super) fn convergence_warnings(v: &View, sol: &mdp::Solution) -> Vec<Warn> {
    sol.unconverged
        .iter()
        .map(|&n| Warn {
            code: "not-converged".into(),
            at: Some(v.g.nodes[n].id.clone()),
            message: "value iteration hit the cap in a cycle: someone can force a costly loop forever, so values here are unreliable (check masks, opponent mode, or add an exit)".into(),
        })
        .collect()
}

/// The best line from `start`: our choices, the opponent's choices, and the
/// most likely draw at chance points.
pub(super) fn best_line(v: &View, sol: &mdp::Solution, start: NodeIx, max: usize) -> Vec<Value> {
    let mut out = vec![];
    let mut cur = start;
    let mut seen = vec![false; v.g.nodes.len()];
    while out.len() < max && !seen[cur] {
        seen[cur] = true;
        let plan = &v.plan[cur];
        if matches!(plan.control, Control::Terminal | Control::Sink) {
            break;
        }
        let dist = chain::step_dist(v, &sol.choice, cur);
        let Some(&(e, p)) = dist.iter().max_by(|a, b| a.1.total_cmp(&b.1)) else {
            break;
        };
        let chosen = sol.choice.get(&cur).copied();
        let kind = match (chosen, plan.control) {
            (Some(c), Control::Opponent) if c == e => "opponent-choice",
            (Some(c), _) if c == e => "choice",
            (Some(WAIT), Control::Opponent) => "opponent-waits-likely-draw",
            (Some(WAIT), _) => "wait-likely-draw",
            _ => "likely-draw",
        };
        let mut j = edge_ref(v, e);
        j["kind"] = json!(kind);
        j["step_p"] = r(p);
        j["q"] = r(sol.q[e]);
        out.push(j);
        cur = v.g.edges[e].to;
    }
    out
}

/// One entry per warning code: count, a representative message, and up to
/// eight locations. Small responses without hiding anything (`lint` lists
/// every location).
pub(super) fn group_warnings(all: Vec<Warn>) -> Vec<super::GroupedWarning> {
    let mut by: BTreeMap<String, super::GroupedWarning> = BTreeMap::new();
    for w in all {
        let g = by.entry(w.code.clone()).or_insert_with(|| super::GroupedWarning { code: w.code.clone(), count: 0, example: w.message.clone(), at: vec![] });
        g.count += 1;
        if let Some(a) = w.at {
            if g.at.len() < 8 {
                g.at.push(a);
            }
        }
    }
    by.into_values().collect()
}
