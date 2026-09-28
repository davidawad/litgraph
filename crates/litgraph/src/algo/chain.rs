// SPDX-License-Identifier: GPL-3.0-or-later
//! Absorbing Markov chain under a fixed policy: "what actually happens in
//! expectation" from a start node — absorption probabilities per terminal,
//! expected visits, and the expected total of ANY list of metrics.
//!
//! Transient states are the non-terminals the policy can actually reach from the start.
//! Transition rows follow the node plans (draws + choice mass on the chosen
//! edge). Solves `(I − Q)ᵀ x = e_start` once by LU (x = expected visits), then
//! every expectation is a dot product. Singular systems (a policy that loops
//! forever) are reported with the offending closed class, not a bare panic.

use serde::Serialize;
use std::collections::BTreeMap;

use crate::algo::mdp::Solution;
use crate::error::{Error, Result};
use crate::model::NodeIx;
use crate::scenario::{Control, View, WAIT};

/// Exact expectations for a fixed policy, from one start node to absorption.
#[derive(Debug, Clone, Serialize)]
pub struct ChainResult {
    /// The node the chain was started from.
    pub start: NodeIx,
    /// terminal -> probability of ending there.
    pub absorption: Vec<(NodeIx, f64)>,
    /// transient node -> expected visits.
    pub visits: Vec<(NodeIx, f64)>,
    /// metric name -> expected total to absorption.
    pub expected: BTreeMap<String, f64>,
    /// Expected number of transitions to absorption.
    pub expected_steps: f64,
    /// Expected terminal utility (Σ P(t)·utility(t)).
    pub expected_utility: f64,
    /// Probability mass that never absorbs (0 unless sinks exist).
    pub sink_mass: f64,
}

/// Distribution over out-edges at node `n` under `choice`.
#[must_use]
pub fn step_dist(v: &View, sol_choice: &BTreeMap<NodeIx, usize>, n: NodeIx) -> Vec<(usize, f64)> {
    let plan = &v.plan[n];
    let mut d: Vec<(usize, f64)> = plan.draws.clone();
    if plan.choice_mass > 0.0 {
        if sol_choice.get(&n) == Some(&WAIT) {
            d.extend(plan.wait.iter().map(|&(e, p)| (e, p * plan.choice_mass)));
        } else if let Some(&e) = sol_choice.get(&n) {
            d.push((e, plan.choice_mass));
        } else if !plan.choices.is_empty() {
            let k = plan.choices.len() as f64;
            d.extend(plan.choices.iter().map(|&e| (e, plan.choice_mass / k)));
        }
    }
    d
}

/// Solves the absorbing chain induced by policy `sol` from `start`: exact
/// absorption probabilities, expected visits, and expected totals for every
/// metric in `metrics` (parallel arrays of per-edge values, indexed like `v.g.edges`).
///
/// # Errors
/// `Numeric` if the induced transition matrix is singular — either some
/// transient state's out-probabilities do not sum to 1, or (more commonly)
/// the policy loops forever from some reachable state without ever
/// absorbing at a terminal.
pub fn chain(
    v: &View,
    sol: &Solution,
    start: NodeIx,
    metrics: &[(String, Vec<f64>)],
) -> Result<ChainResult> {
    // Transient states = non-terminals reachable from `start` along edges the
    // policy actually uses. (Graph-reachable is wrong: a state the policy
    // never visits may loop on itself and make I − Q singular.)
    let mut reach = vec![false; v.g.nodes.len()];
    let mut stack = vec![start];
    reach[start] = true;
    while let Some(u) = stack.pop() {
        if matches!(v.plan[u].control, Control::Terminal) {
            continue;
        }
        for (e, p) in step_dist(v, &sol.choice, u) {
            let w = v.g.edges[e].to;
            if p > 0.0 && !reach[w] {
                reach[w] = true;
                stack.push(w);
            }
        }
    }
    let transient: Vec<NodeIx> = (0..v.g.nodes.len())
        .filter(|&n| reach[n] && !matches!(v.plan[n].control, Control::Terminal))
        .collect();
    let tix: BTreeMap<NodeIx, usize> = transient.iter().enumerate().map(|(i, &n)| (n, i)).collect();
    let m = transient.len();
    if v.plan[start].control == Control::Terminal {
        return Ok(ChainResult {
            start,
            absorption: vec![(start, 1.0)],
            visits: vec![],
            expected: metrics.iter().map(|(k, _)| (k.clone(), 0.0)).collect(),
            expected_steps: 0.0,
            expected_utility: v.utility[start],
            sink_mass: 0.0,
        });
    }
    // A = (I − Q)ᵀ, dense.
    let mut a = vec![0.0; m * m];
    for i in 0..m {
        a[i * m + i] = 1.0;
    }
    let mut dists = Vec::with_capacity(m);
    for (i, &n) in transient.iter().enumerate() {
        let d = step_dist(v, &sol.choice, n);
        for &(e, p) in &d {
            if let Some(&j) = tix.get(&v.g.edges[e].to) {
                a[j * m + i] -= p; // transpose
            }
        }
        dists.push(d);
    }
    let mut b = vec![0.0; m];
    b[tix[&start]] = 1.0;
    let x = lu_solve(&mut a, m, &mut b).map_err(|_| {
        // Trapped states: transient nodes from which no terminal is reachable
        // along edges the policy actually uses (positive probability).
        let mut escapes = vec![false; m];
        let mut changed = true;
        while changed {
            changed = false;
            for i in 0..m {
                if escapes[i] {
                    continue;
                }
                let out = dists[i].iter().any(|&(e, p)| p > 0.0 && tix.get(&v.g.edges[e].to).is_none_or(|&j| escapes[j]));
                if out {
                    escapes[i] = true;
                    changed = true;
                }
            }
        }
        let trapped: Vec<String> = (0..m)
            .filter(|&i| !escapes[i] && x_reachable(&dists, &tix, v, i, tix[&start]))
            .map(|i| {
                let n = transient[i];
                let via = sol.choice.get(&n).map(|&e| format!(" (chooses `{}`)", crate::api::choice_label(v, e))).unwrap_or_default();
                format!("{}{via}", v.g.nodes[n].id)
            })
            .take(12)
            .collect();
        let bad_rows: Vec<String> = (0..m)
            .filter_map(|i| {
                let s: f64 = dists[i].iter().map(|x| x.1).sum();
                ((s - 1.0).abs() > 1e-9).then(|| format!("{} (out-probability {s:.6})", v.g.nodes[transient[i]].id))
            })
            .take(8)
            .collect();
        if trapped.is_empty() && !bad_rows.is_empty() {
            return Error::Numeric(format!("transition rows do not sum to 1: {}", bad_rows.join("; ")));
        }
        Error::Numeric(format!("policy never terminates: these states cannot reach a terminal under the chosen policy: {}", trapped.join("; ")))
    })?;
    let mut absorption: BTreeMap<NodeIx, f64> = BTreeMap::new();
    let mut expected: BTreeMap<String, f64> =
        metrics.iter().map(|(k, _)| (k.clone(), 0.0)).collect();
    let mut steps = 0.0;
    let mut sink_mass = 0.0;
    for (i, &n) in transient.iter().enumerate() {
        let visits = x[i];
        steps += visits;
        if v.plan[n].control == Control::Sink {
            sink_mass += visits;
        }
        for &(e, p) in &dists[i] {
            let to = v.g.edges[e].to;
            if !tix.contains_key(&to) {
                *absorption.entry(to).or_default() += visits * p;
            }
            for (k, vals) in metrics {
                let val = vals[e];
                if val.is_finite() {
                    *expected.get_mut(k).unwrap() += visits * p * val;
                }
            }
        }
    }
    let expected_utility = absorption.iter().map(|(&t, &p)| p * v.utility[t]).sum();
    let mut absorption: Vec<(NodeIx, f64)> =
        absorption.into_iter().filter(|(_, p)| *p > 1e-12).collect();
    absorption.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut visits: Vec<(NodeIx, f64)> = transient
        .iter()
        .enumerate()
        .map(|(i, &n)| (n, x[i]))
        .filter(|(_, x)| *x > 1e-12)
        .collect();
    visits.sort_by(|a, b| b.1.total_cmp(&a.1));
    Ok(ChainResult {
        start,
        absorption,
        visits,
        expected,
        expected_steps: steps,
        expected_utility,
        sink_mass,
    })
}

/// Is transient `i` reachable from transient `s` along positive-probability policy edges?
fn x_reachable(
    dists: &[Vec<(usize, f64)>],
    tix: &BTreeMap<NodeIx, usize>,
    v: &View,
    i: usize,
    s: usize,
) -> bool {
    let mut seen = vec![false; dists.len()];
    let mut stack = vec![s];
    seen[s] = true;
    while let Some(u) = stack.pop() {
        if u == i {
            return true;
        }
        for &(e, p) in &dists[u] {
            if p <= 0.0 {
                continue;
            }
            if let Some(&j) = tix.get(&v.g.edges[e].to) {
                if !seen[j] {
                    seen[j] = true;
                    stack.push(j);
                }
            }
        }
    }
    false
}

/// In-place LU with partial pivoting; solves A x = b. Err on singular.
pub fn lu_solve(a: &mut [f64], n: usize, b: &mut [f64]) -> std::result::Result<Vec<f64>, ()> {
    for col in 0..n {
        let mut piv = col;
        for r in col + 1..n {
            if a[r * n + col].abs() > a[piv * n + col].abs() {
                piv = r;
            }
        }
        if a[piv * n + col].abs() < 1e-12 {
            return Err(());
        }
        if piv != col {
            for j in 0..n {
                a.swap(col * n + j, piv * n + j);
            }
            b.swap(col, piv);
        }
        let d = a[col * n + col];
        for r in col + 1..n {
            let f = a[r * n + col] / d;
            if f == 0.0 {
                continue;
            }
            for j in col..n {
                a[r * n + j] -= f * a[col * n + j];
            }
            b[r] -= f * b[col];
        }
    }
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        let mut s = b[i];
        for j in i + 1..n {
            s -= a[i * n + j] * x[j];
        }
        x[i] = s / a[i * n + i];
    }
    Ok(x)
}
