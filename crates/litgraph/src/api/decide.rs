// SPDX-License-Identifier: GPL-3.0-or-later
//! Decision ops: solve, explain, chain, simulate.

use serde_json::{json, Map, Value};

use super::render::{best_line, choice_id, choice_label, convergence_warnings, edge_ref, metric_list, node_ref, r};
use super::Warn;
use crate::algo::{chain, mdp, sim};
use crate::error::Result;
use crate::model::NodeIx;
use crate::scenario::{Control, View, WAIT};

type Out = Result<(Value, Vec<Warn>)>;

fn solve(v: &View) -> Result<mdp::Solution> {
    mdp::solve(v, &mdp::SolveOptions::default())
}

pub(super) fn solve_op(v: &View, start: NodeIx, full_policy: bool, all_values: bool, max_steps: usize) -> Out {
    let sol = solve(v)?;
    let policy: Vec<Value> = sol
        .my_policy(v)
        .map(|(n, e)| json!({ "node": v.g.nodes[n].id, "edge": choice_id(v, e), "label": choice_label(v, e), "q": r(sol.option_q(v, n, e)) }))
        .collect();
    let values = all_values.then(|| (0..v.g.nodes.len()).map(|n| (v.g.nodes[n].id.clone(), r(sol.value[n]))).collect::<Map<_, _>>());
    let result = json!({
        "start": node_ref(v, start),
        "value": r(sol.value[start]),
        "converged": sol.converged,
        "iterations": sol.iterations,
        "fee_shift_rounds": sol.fee_shift_rounds,
        "best_line": best_line(v, &sol, start, max_steps),
        "policy_size": policy.len(),
        "policy": full_policy.then_some(policy),
        "values": values,
    });
    Ok((result, convergence_warnings(v, &sol)))
}

pub(super) fn chain_op(v: &View, start: NodeIx, metrics: &[String], top: usize) -> Out {
    let sol = solve(v)?;
    let ms = metric_list(v, metrics)?;
    let c = chain::chain(v, &sol, start, &ms)?;
    let cost_total = metrics.first().and_then(|m| c.expected.get(m)).copied().unwrap_or(0.0);
    let absorption: Vec<Value> = c
        .absorption
        .iter()
        .map(|&(t, p)| {
            let n = &v.g.nodes[t];
            json!({ "terminal": n.id, "label": n.label, "p": r(p), "utility": r(v.utility[t]), "outcome": n.outcome })
        })
        .collect();
    let result = json!({
        "start": node_ref(v, start),
        "absorption": absorption,
        "expected": c.expected.iter().map(|(k, x)| (k.clone(), r(*x))).collect::<Map<_, _>>(),
        "expected_steps": r(c.expected_steps),
        "expected_utility": r(c.expected_utility),
        "expected_net": r(c.expected_utility - cost_total),
        "sink_mass": r(c.sink_mass),
        "top_visits": c.visits.iter().take(top).map(|&(n, x)| json!({ "node": v.g.nodes[n].id, "visits": r(x) })).collect::<Vec<_>>(),
    });
    Ok((result, convergence_warnings(v, &sol)))
}

pub(super) fn simulate_op(v: &View, start: NodeIx, metrics: &[String], o: &sim::SimOptions) -> Out {
    let sol = solve(v)?;
    let ms = metric_list(v, metrics)?;
    let res = sim::simulate(v, &sol, start, &ms, o)?;
    let result = json!({
        "start": node_ref(v, start),
        "runs": res.runs,
        "seed": res.seed,
        "net": res.net,
        "cvar": r(res.cvar),
        "alpha": res.alpha,
        "p_loss": r(res.p_loss),
        "metrics": res.metrics,
        "truncated": res.truncated,
        "terminals": res.terminals.iter().map(|&(t, p)| json!({ "terminal": v.g.nodes[t].id, "p": r(p) })).collect::<Vec<_>>(),
        "samples": res.samples.iter().map(|tr| tr.iter().map(|&e| v.g.edges[e].label.clone()).collect::<Vec<_>>()).collect::<Vec<_>>(),
    });
    Ok((result, convergence_warnings(v, &sol)))
}

fn who_decides(control: Control, minimize: bool) -> &'static str {
    match (control, minimize) {
        (Control::Me, _) => "you",
        (Control::Opponent, true) => "opponent (adversarial)",
        (Control::Opponent, false) => "opponent",
        (Control::Chance, _) => "tribunal/chance",
        (Control::Terminal, _) => "nobody (terminal)",
        (Control::Sink, _) => "nobody (dead end)",
    }
}

fn option_json(v: &View, sol: &mdp::Solution, n: NodeIx, e: usize) -> Value {
    let plan = &v.plan[n];
    let ed = &v.g.edges[e];
    let q = sol.q[e];
    let mut j = edge_ref(v, e);
    j["q"] = r(q);
    j["cost"] = r(v.cost[e]);
    j["hours"] = r(ed.hours);
    j["fees"] = r(ed.cost);
    if let Some(du) = &ed.duration {
        j["elapsed_days"] = json!(du);
    }
    if !ed.tags.is_empty() {
        j["tags"] = json!(ed.tags);
    }
    if let Some(nt) = &ed.note {
        j["note"] = json!(nt);
    }
    let best = sol.choice.get(&n).copied();
    j["chosen"] = json!(best == Some(e));
    let kind = if plan.choices.contains(&e) {
        "choice"
    } else if plan.wait.iter().any(|&(w, _)| w == e) {
        "draw-if-waiting"
    } else {
        "draw"
    };
    j["kind"] = json!(kind);
    if let (Some(b), "choice") = (best, kind) {
        let qb = sol.option_q(v, n, b);
        j["regret"] = r(if plan.minimize { q - qb } else { qb - q });
    }
    // What happens after taking this edge: absorption from its target under the policy.
    if let Ok(after) = chain::chain(v, sol, ed.to, &[]) {
        j["then"] = json!(after.absorption.iter().take(4).map(|&(t, p)| json!({ "terminal": v.g.nodes[t].local_id, "p": r(p) })).collect::<Vec<_>>());
    }
    j
}

pub(super) fn explain_op(v: &View, n: NodeIx) -> Out {
    let sol = solve(v)?;
    let node = &v.g.nodes[n];
    let plan = &v.plan[n];
    let mut options: Vec<(f64, Value)> = v.outs(n).map(|e| (sol.q[e], option_json(v, &sol, n, e))).collect();
    if !plan.wait.is_empty() {
        let qw = sol.option_q(v, n, WAIT);
        let best = sol.choice.get(&n).copied();
        let mut j = json!({
            "id": "WAIT", "label": choice_label(v, WAIT), "kind": "wait", "q": r(qw), "chosen": best == Some(WAIT),
            "draws": plan.wait.iter().map(|&(e, p)| json!({ "edge": v.g.edges[e].id, "label": v.g.edges[e].label, "p": r(p) })).collect::<Vec<_>>(),
        });
        if let Some(b) = best {
            let qb = sol.option_q(v, n, b);
            j["regret"] = r(if plan.minimize { qw - qb } else { qb - qw });
        }
        options.push((qw, j));
    }
    options.sort_by(|a, b| b.0.total_cmp(&a.0));
    let here: Vec<_> = v.warnings.iter().filter(|w| w.at.as_deref() == Some(node.id.as_str())).collect();
    let result = json!({
        "node": { "id": node.id, "label": node.label, "kind": node.kind, "cite": node.cite, "note": node.note },
        "control": plan.control,
        "who_decides": who_decides(plan.control, plan.minimize),
        "value": r(sol.value[n]),
        "interrupt_mass": r(1.0 - plan.choice_mass),
        "options": options.into_iter().map(|x| x.1).collect::<Vec<_>>(),
        "warnings_here": here,
    });
    Ok((result, convergence_warnings(v, &sol)))
}
