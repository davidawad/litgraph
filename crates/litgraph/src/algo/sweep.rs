// SPDX-License-Identifier: GPL-3.0-or-later
//! Parameter sweeps, policy-flip breakpoints, and sensitivity (tornado).
//!
//! Generalizes v1's policy-diff: any scenario parameter — including ones only
//! a custom metric/utility reads — can be swept, and every node whose optimal
//! choice changes is refined to its breakpoint by bisection.

use serde::Serialize;
use std::collections::BTreeMap;

use crate::algo::mdp::{solve, SolveOptions};
use crate::error::Result;
use crate::model::{Graph, NodeIx};
use crate::scenario::{Control, Scenario, View};

/// A parameter value where a watched node's optimal choice flips.
#[derive(Debug, Clone, Serialize)]
pub struct Breakpoint {
    /// The node whose optimal choice flips.
    pub node: NodeIx,
    /// Parameter value where the choice flips (± tolerance).
    pub at: f64,
    /// Chosen out-edge index just before the flip.
    pub before: usize,
    /// Chosen out-edge index just after the flip.
    pub after: usize,
}

/// Result of sweeping one parameter over a grid.
#[derive(Debug, Clone, Serialize)]
pub struct SweepResult {
    /// The parameter that was swept.
    pub param: String,
    /// (param value, V(start)).
    pub curve: Vec<(f64, f64)>,
    /// Every policy flip found, refined by bisection and sorted by `at`.
    pub breakpoints: Vec<Breakpoint>,
}

fn policy_at(
    g: &Graph,
    sc: &Scenario,
    param: &str,
    x: f64,
    start: Option<NodeIx>,
) -> Result<(f64, BTreeMap<NodeIx, usize>)> {
    let mut sc = sc.clone();
    sc.params.insert(param.to_string(), x);
    let v = View::new(g, &sc)?;
    let s = solve(&v, &SolveOptions::default())?;
    let pol = s.my_policy(&v).collect();
    Ok((s.value[start.unwrap_or(v.start)], pol))
}

/// What to sweep.
#[derive(Debug, Clone, Copy)]
pub struct SweepSpec<'a> {
    /// Parameter name (any; custom functions may read it).
    pub param: &'a str,
    /// Low end.
    pub lo: f64,
    /// High end.
    pub hi: f64,
    /// Grid points (at least 2).
    pub steps: usize,
    /// Nodes whose choice flips are reported (empty = all).
    pub watch: &'a [NodeIx],
    /// Bisection tolerance on the parameter.
    pub tol: f64,
}

/// Solve on a grid of parameter values and refine every policy flip to its breakpoint.
///
/// # Errors
/// View or solve errors at any grid point.
pub fn sweep(g: &Graph, sc: &Scenario, spec: &SweepSpec<'_>) -> Result<SweepResult> {
    let SweepSpec { param, lo, hi, steps, watch, tol } = *spec;
    let steps = steps.max(2);
    let xs: Vec<f64> = (0..steps)
        .map(|i| lo + (hi - lo) * i as f64 / (steps - 1) as f64)
        .collect();
    let mut curve = vec![];
    let mut pols = vec![];
    for &x in &xs {
        let (val, pol) = policy_at(g, sc, param, x, None)?;
        curve.push((x, val));
        pols.push(pol);
    }
    let mut breakpoints = vec![];
    for i in 1..xs.len() {
        let (a, b) = (&pols[i - 1], &pols[i]);
        for (&n, &ea) in a {
            if !watch.is_empty() && !watch.contains(&n) {
                continue;
            }
            let Some(&eb) = b.get(&n) else { continue };
            if ea == eb {
                continue;
            }
            // Bisect on this node.
            let (mut l, mut r) = (xs[i - 1], xs[i]);
            let mut er = eb;
            while r - l > tol {
                let m = 0.5 * (l + r);
                let (_, pm) = policy_at(g, sc, param, m, None)?;
                match pm.get(&n) {
                    Some(&em) if em == ea => l = m,
                    Some(&em) => {
                        r = m;
                        er = em;
                    }
                    None => break,
                }
            }
            breakpoints.push(Breakpoint {
                node: n,
                at: 0.5 * (l + r),
                before: ea,
                after: er,
            });
        }
    }
    breakpoints.sort_by(|a, b| a.at.total_cmp(&b.at));
    Ok(SweepResult {
        param: param.to_string(),
        curve,
        breakpoints,
    })
}

/// One row of a tornado analysis: how much `V(start)` swings when one input
/// is perturbed to its low/high band.
#[derive(Debug, Clone, Serialize)]
pub struct Sensitivity {
    /// What was perturbed: `param:<name>` or `p:<edge id>`.
    pub input: String,
    /// Low end of the perturbation band.
    pub low_value: f64,
    /// High end of the perturbation band.
    pub high_value: f64,
    /// V(start) at the low / high perturbation.
    pub v_low: f64,
    /// V(start) at the high perturbation.
    pub v_high: f64,
    /// `|v_high - v_low|`: how much this input drives the answer.
    pub swing: f64,
    /// Did the first-move policy change within the band?
    pub policy_changes: bool,
}

/// Tornado: perturb each listed parameter by ±`rel` and each authored draw
/// probability by ±`dp` (siblings rescaled), rank by swing in V(start).
///
/// # Errors
/// View or solve errors at any perturbation.
pub fn tornado(
    g: &Graph,
    sc: &Scenario,
    params: &[String],
    rel: f64,
    dp: f64,
    include_probabilities: bool,
) -> Result<(f64, Vec<Sensitivity>)> {
    let base_v = View::new(g, sc)?;
    let base = solve(&base_v, &SolveOptions::default())?;
    let start = base_v.start;
    let base_val = base.value[start];
    let base_pol: BTreeMap<NodeIx, usize> = base.my_policy(&base_v).collect();
    let eval = |sc2: &Scenario| -> Result<(f64, bool)> {
        let v = View::new(g, sc2)?;
        let s = solve(&v, &SolveOptions::default())?;
        let pol: BTreeMap<NodeIx, usize> = s.my_policy(&v).collect();
        Ok((s.value[v.start], pol != base_pol))
    };
    let mut out = vec![];
    for p in params {
        let x = base_v.params.get(p).copied().unwrap_or(0.0);
        let (lo, hi) = if x == 0.0 {
            (-rel, rel)
        } else {
            (x * (1.0 - rel), x * (1.0 + rel))
        };
        let mut a = sc.clone();
        a.params.insert(p.clone(), lo);
        let mut b = sc.clone();
        b.params.insert(p.clone(), hi);
        let ((vl, cl), (vh, ch)) = (eval(&a)?, eval(&b)?);
        out.push(Sensitivity {
            input: format!("param:{p}"),
            low_value: lo,
            high_value: hi,
            v_low: vl,
            v_high: vh,
            swing: (vh - vl).abs(),
            policy_changes: cl || ch,
        });
    }
    if include_probabilities {
        for e in 0..g.edges.len() {
            let from = g.edges[e].from;
            if !base_v.active[e] || base_v.plan[from].control == Control::Terminal {
                continue;
            }
            let Some(p0) = base_v.prob[e] else { continue };
            if g.edges[e].probability.is_none() {
                continue;
            }
            // Skip binary-node duplicates: perturbing one side of a 2-way draw is the other side mirrored.
            let draws = base_v.plan[from].draws.len();
            if draws == 2 && base_v.plan[from].draws[1].0 == e {
                continue;
            }
            let (lo, hi) = ((p0 - dp).max(0.0), (p0 + dp).min(1.0));
            let mut a = sc.clone();
            a.probabilities.insert(g.edges[e].id.clone(), lo);
            let mut b = sc.clone();
            b.probabilities.insert(g.edges[e].id.clone(), hi);
            let ((vl, cl), (vh, ch)) = (eval(&a)?, eval(&b)?);
            out.push(Sensitivity {
                input: format!("p:{}", g.edges[e].id),
                low_value: lo,
                high_value: hi,
                v_low: vl,
                v_high: vh,
                swing: (vh - vl).abs(),
                policy_changes: cl || ch,
            });
        }
    }
    out.sort_by(|a, b| b.swing.total_cmp(&a.swing));
    Ok((base_val, out))
}
