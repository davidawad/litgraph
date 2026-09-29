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
        } else {
            // No recorded choice: spread the mass evenly (a no-op if there are
            // no choices, so `k` is never used as a zero divisor).
            let k = plan.choices.len() as f64;
            d.extend(plan.choices.iter().map(|&e| (e, plan.choice_mass / k)));
        }
    }
    d
}

/// Transient states = non-terminals reachable from `start` along edges the
/// policy actually uses. (Graph-reachable is wrong: a state the policy
/// never visits may loop on itself and make I − Q singular.)
fn transient_states(v: &View, sol: &Solution, start: NodeIx) -> Vec<NodeIx> {
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
    (0..v.g.nodes.len())
        .filter(|&n| reach[n] && !matches!(v.plan[n].control, Control::Terminal))
        .collect()
}

/// Builds `A = (I − Q)ᵀ` (dense, row-major) plus each transient state's
/// step distribution, for the absorbing-chain linear system.
fn build_system(
    v: &View,
    sol: &Solution,
    transient: &[NodeIx],
    tix: &BTreeMap<NodeIx, usize>,
) -> (Vec<f64>, Vec<Vec<(usize, f64)>>) {
    let m = transient.len();
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
    (a, dists)
}

/// Explains a singular `(I − Q)ᵀ`: either transition rows that do not sum to
/// 1, or transient states that cannot reach a terminal under this policy.
fn diagnose_singular(
    v: &View,
    sol: &Solution,
    transient: &[NodeIx],
    tix: &BTreeMap<NodeIx, usize>,
    dists: &[Vec<(usize, f64)>],
    start_ix: usize,
) -> Error {
    let m = transient.len();
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
            let out = dists[i]
                .iter()
                .any(|&(e, p)| p > 0.0 && tix.get(&v.g.edges[e].to).is_none_or(|&j| escapes[j]));
            if out {
                escapes[i] = true;
                changed = true;
            }
        }
    }
    let trapped: Vec<String> = (0..m)
        .filter(|&i| !escapes[i] && x_reachable(dists, tix, v, i, start_ix))
        .map(|i| {
            let n = transient[i];
            let via = sol
                .choice
                .get(&n)
                .map(|&e| format!(" (chooses `{}`)", crate::api::choice_label(v, e)))
                .unwrap_or_default();
            format!("{}{via}", v.g.nodes[n].id)
        })
        .take(12)
        .collect();
    let bad_rows: Vec<String> = (0..m)
        .filter_map(|i| {
            let s: f64 = dists[i].iter().map(|x| x.1).sum();
            ((s - 1.0).abs() > 1e-9)
                .then(|| format!("{} (out-probability {s:.6})", v.g.nodes[transient[i]].id))
        })
        .take(8)
        .collect();
    if trapped.is_empty() && !bad_rows.is_empty() {
        return Error::Numeric(format!(
            "transition rows do not sum to 1: {}",
            bad_rows.join("; ")
        ));
    }
    Error::Numeric(format!(
        "policy never terminates: these states cannot reach a terminal under the chosen policy: {}",
        trapped.join("; ")
    ))
}

/// Tallies absorption probabilities, expected visits, and expected metric
/// totals from the solved expected-visit vector `x`.
fn tally(
    v: &View,
    transient: &[NodeIx],
    tix: &BTreeMap<NodeIx, usize>,
    dists: &[Vec<(usize, f64)>],
    x: &[f64],
    metrics: &[(String, Vec<f64>)],
) -> (BTreeMap<NodeIx, f64>, BTreeMap<String, f64>, f64, f64) {
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
                    *expected.entry(k.clone()).or_insert(0.0) += visits * p * val;
                }
            }
        }
    }
    (absorption, expected, steps, sink_mass)
}

/// Solves the absorbing chain induced by policy `sol` from `start`: exact
/// absorption probabilities, expected visits, and expected totals for every
/// metric in `metrics` (parallel arrays of per-edge values, indexed like `v.g.edges`).
///
/// ```
/// use litgraph::algo::{chain, mdp};
/// use litgraph::model::{CompileOptions, Graph, LinkFile, Pack};
/// use litgraph::scenario::{Scenario, View};
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
/// let v = View::new(&g, &Scenario::default()).unwrap();
/// let sol = mdp::solve(&v, &mdp::SolveOptions::default()).unwrap();
/// let c = chain::chain(&v, &sol, v.start, &[]).unwrap();
/// // The only line absorbs at `end` with probability 1.
/// assert_eq!(c.absorption, vec![(g.node("demo::end").unwrap(), 1.0)]);
/// assert_eq!(c.expected_utility, 100.0);
/// ```
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
    let transient = transient_states(v, sol, start);
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
    let (mut a, dists) = build_system(v, sol, &transient, &tix);
    let mut b = vec![0.0; m];
    b[tix[&start]] = 1.0;
    let x = lu_solve(&mut a, m, &mut b)
        .map_err(|Singular| diagnose_singular(v, sol, &transient, &tix, &dists, tix[&start]))?;
    let (absorption, expected, steps, sink_mass) = tally(v, &transient, &tix, &dists, &x, metrics);
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
        // Positive-probability steps to other transient nodes.
        let next = dists[u]
            .iter()
            .filter(|&&(_, p)| p > 0.0)
            .filter_map(|&(e, _)| tix.get(&v.g.edges[e].to).copied());
        for j in next {
            if !seen[j] {
                seen[j] = true;
                stack.push(j);
            }
        }
    }
    false
}

/// Marker error: the system was numerically singular (a zero pivot within
/// tolerance). Carries no data — [`chain`] turns it into a diagnostic that
/// names the offending states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Singular;

/// In-place LU decomposition with partial pivoting; solves `A x = b`.
///
/// # Errors
/// [`Singular`] if a pivot is zero (within tolerance) — the matrix has no
/// unique solution.
pub fn lu_solve(a: &mut [f64], n: usize, b: &mut [f64]) -> std::result::Result<Vec<f64>, Singular> {
    for col in 0..n {
        let mut piv = col;
        for r in col + 1..n {
            if a[r * n + col].abs() > a[piv * n + col].abs() {
                piv = r;
            }
        }
        if a[piv * n + col].abs() < 1e-12 {
            return Err(Singular);
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::model::{CompileOptions, Graph, LinkFile, Pack};
    use crate::scenario::{Scenario, View};

    fn compile(j: serde_json::Value) -> Graph {
        let p: Pack = serde_json::from_value(j).expect("well-formed test pack");
        Graph::compile(&[p], &LinkFile::default(), &CompileOptions::default())
            .expect("test pack compiles")
    }

    /// A fabricated transition row whose probabilities don't sum to 1 (a
    /// caller bug, not a real policy) must be diagnosed as exactly that,
    /// not blamed on a non-terminating policy: its edge target is a
    /// terminal (outside the transient set), so it "escapes" immediately
    /// and can never appear in the trapped-states list either way.
    #[test]
    fn diagnose_singular_reports_rows_that_do_not_sum_to_one() {
        let g = compile(serde_json::json!({
            "schemaVersion": 2, "id": "ds", "title": "ds", "startNodeId": "s",
            "nodes": [
                { "id": "s", "kind": "state", "label": "s" },
                { "id": "e", "kind": "terminal", "label": "e", "payoff": 1 }
            ],
            "edges": [{ "id": "go", "from": "s", "to": "e", "label": "go", "actor": "applicant" }]
        }));
        let v = View::new(&g, &Scenario::default()).unwrap();
        let sol = crate::algo::mdp::solve(&v, &crate::algo::mdp::SolveOptions::default()).unwrap();
        let s_ix = g.node("ds::s").unwrap();
        let transient = vec![s_ix];
        let tix: BTreeMap<NodeIx, usize> = BTreeMap::from([(s_ix, 0)]);
        // 0.4 instead of 1.0: a fabricated bad row (this never happens from
        // a real `step_dist` call, which is exactly why this diagnostic
        // needs its own direct test rather than one routed through a real
        // solve).
        let dists = vec![vec![(g.edge("go").unwrap(), 0.4)]];
        let err = diagnose_singular(&v, &sol, &transient, &tix, &dists, 0);
        assert_eq!(err.code(), "numeric");
        assert!(err.to_string().contains("do not sum to 1"), "{err}");
    }

    /// `x_reachable` walks positive-probability edges only, and reports
    /// `false` for a target with no path at all — both need a fabricated
    /// `dists` table, since a real solved policy's transient set is built
    /// from exactly the same reachability this function re-checks (so a
    /// real call can never see an unreachable target here).
    #[test]
    fn x_reachable_skips_zero_probability_edges_and_reports_unreachable_targets() {
        let g = compile(serde_json::json!({
            "schemaVersion": 2, "id": "xr", "title": "xr", "startNodeId": "s",
            "nodes": [
                { "id": "s", "kind": "state", "label": "s" },
                { "id": "a", "kind": "state", "label": "a" },
                { "id": "b", "kind": "state", "label": "b" },
                { "id": "isolated", "kind": "state", "label": "isolated" }
            ],
            "edges": [
                { "id": "s-zero", "from": "s", "to": "a", "label": "s-zero", "actor": "either" },
                { "id": "s-b", "from": "s", "to": "b", "label": "s-b", "actor": "either" }
            ]
        }));
        let v = View::new(&g, &Scenario::default()).unwrap();
        let s_ix = g.node("xr::s").unwrap();
        let a_ix = g.node("xr::a").unwrap();
        let b_ix = g.node("xr::b").unwrap();
        let isolated_ix = g.node("xr::isolated").unwrap();
        let tix: BTreeMap<NodeIx, usize> =
            BTreeMap::from([(s_ix, 0), (a_ix, 1), (b_ix, 2), (isolated_ix, 3)]);
        let dists = vec![
            // From s: a zero-probability edge to `a` (skipped) and a
            // positive-probability edge to `b` (followed).
            vec![
                (g.edge("s-zero").unwrap(), 0.0),
                (g.edge("s-b").unwrap(), 1.0),
            ],
            vec![], // a: no out-edges recorded
            vec![], // b: no out-edges recorded
            vec![], // isolated: unreachable from s no matter what
        ];
        // `a` is only reachable via the zero-probability edge: unreachable.
        assert!(!x_reachable(&dists, &tix, &v, 1, 0));
        // `b` is reachable via the positive-probability edge.
        assert!(x_reachable(&dists, &tix, &v, 2, 0));
        // `isolated` has no edge into it at all from `s`.
        assert!(!x_reachable(&dists, &tix, &v, 3, 0));
    }
}
