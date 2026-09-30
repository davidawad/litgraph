// SPDX-License-Identifier: GPL-3.0-or-later
//! Each side's continuation value under a fixed (solved) policy: the
//! certainty equivalent of litigating on from a node, under that side's own
//! payoffs, costs and risk attitude.

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use std::collections::BTreeMap;

use crate::algo::chain::step_dist;
use crate::algo::mdp::{aggregate, gamma};
use crate::algo::structure::{is_cyclic, scc};
use crate::model::NodeIx;
use crate::scenario::{Control, Objective, View};

/// A side's risk attitude toward the litigation lottery.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Risk {
    /// Risk-neutral: the expectation.
    Expected,
    /// Exponential utility with absolute risk aversion `a` (exact).
    Cara(f64),
    /// Every draw goes against this side.
    Worst,
    /// Mean of the worst `alpha` fraction (Monte Carlo under the policy).
    Cvar(f64),
}

impl Risk {
    /// The risk attitude an [`Objective`] names.
    #[must_use]
    pub fn of(o: &Objective) -> Risk {
        match *o {
            Objective::Expected => Risk::Expected,
            Objective::Cara { a } => Risk::Cara(a),
            Objective::Worst => Risk::Worst,
            Objective::Cvar { alpha, .. } => Risk::Cvar(alpha),
        }
    }

    /// The CARA coefficient of this side's utility for a sure payment (`0`
    /// for everything but CARA: a sure price has no risk to price).
    #[must_use]
    pub fn cara(self) -> f64 {
        match self {
            Risk::Cara(a) => a,
            _ => 0.0,
        }
    }

    /// The recursively-computable stand-in: `CVaR` is not a backward
    /// recursion under a fixed policy, so exact passes use the expectation.
    pub(crate) fn recursive(self) -> Risk {
        match self {
            Risk::Cvar(_) => Risk::Expected,
            r => r,
        }
    }

    pub(crate) fn aggregate(self, terms: &[(f64, f64)]) -> f64 {
        match self {
            Risk::Cara(a) => aggregate(Some(a), false, terms),
            Risk::Worst => aggregate(None, true, terms),
            Risk::Expected | Risk::Cvar(_) => aggregate(None, false, terms),
        }
    }
}

/// One side's payoffs: a value per terminal and a cost per edge, both in
/// that side's own dollars (higher is better for it).
#[derive(Debug, Clone)]
pub struct Party {
    /// Terminal value (0 off-terminal).
    pub terminal: Vec<f64>,
    /// Cost per edge (NaN on inactive edges, never read).
    pub cost: Vec<f64>,
    /// Risk attitude.
    pub risk: Risk,
}

/// Values for every node by one SCC-ordered pass: acyclic nodes get one
/// backup, cyclic components iterate to a relative `1e-9` fixed point.
/// Returns the values and whether every cycle converged.
pub(crate) fn fixed_point(
    v: &View,
    mut backup: impl FnMut(NodeIx, &[f64]) -> f64,
) -> (Vec<f64>, bool) {
    let mut x = vec![0.0; v.g.nodes.len()];
    let mut converged = true;
    for comp in scc(v) {
        let cyclic = is_cyclic(v, &comp);
        let mut it = 0;
        loop {
            it += 1;
            let mut delta: f64 = 0.0;
            for &u in &comp {
                let new = backup(u, &x);
                delta = delta.max((new - x[u]).abs());
                x[u] = new;
            }
            let scale = comp.iter().map(|&u| x[u].abs()).fold(1.0, f64::max);
            if !cyclic || delta <= 1e-9 * scale {
                break;
            }
            if it >= 100_000 {
                converged = false;
                break;
            }
        }
    }
    (x, converged)
}

/// The side's certainty equivalent at every node under the fixed `choice`
/// map (`CVaR` sides get their expectation here; see [`cvar_at`]).
pub(crate) fn evaluate(
    v: &View,
    choice: &BTreeMap<NodeIx, usize>,
    party: &Party,
) -> (Vec<f64>, bool) {
    let risk = party.risk.recursive();
    fixed_point(v, |n, x| match v.plan[n].control {
        Control::Terminal => party.terminal[n],
        Control::Sink => 0.0,
        _ => {
            let terms: Vec<(f64, f64)> = step_dist(v, choice, n)
                .into_iter()
                .map(|(e, p)| (p, -party.cost[e] + gamma(v, e) * x[v.g.edges[e].to]))
                .collect();
            risk.aggregate(&terms)
        }
    })
}

/// Expected calendar days until the next procedural step that takes time:
/// zero-duration forks are looked through to the first timed step behind
/// them. This is the length of one bargaining round.
pub(crate) fn round_days(v: &View, choice: &BTreeMap<NodeIx, usize>) -> Vec<f64> {
    fixed_point(v, |n, x| match v.plan[n].control {
        Control::Terminal | Control::Sink => 0.0,
        _ => step_dist(v, choice, n)
            .into_iter()
            .map(|(e, p)| {
                let d = v.elapsed[e];
                p * if d > 0.0 { d } else { x[v.g.edges[e].to] }
            })
            .sum(),
    })
    .0
}

/// Sampling budget for the `CVaR` Monte Carlo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sampling {
    /// Trajectories per node.
    pub runs: usize,
    /// RNG seed (the estimate is deterministic given the seed).
    pub seed: u64,
}

/// Per-run step cap for the `CVaR` Monte Carlo.
const MAX_RUN_STEPS: usize = 10_000;

/// `CVaR_alpha` of each party's total outcome from `n` under the policy, by
/// Monte Carlo (sorted samples, mean of the worst `ceil(alpha·runs)`).
/// Returns the per-party estimates and the number of truncated runs.
pub(crate) fn cvar_at(
    v: &View,
    choice: &BTreeMap<NodeIx, usize>,
    parties: &[&Party],
    n: NodeIx,
    s: Sampling,
) -> (Vec<f64>, usize) {
    let mut rng = ChaCha8Rng::seed_from_u64(s.seed ^ (n as u64));
    let runs = s.runs.max(1);
    let mut samples = vec![Vec::with_capacity(runs); parties.len()];
    let mut truncated = 0;
    for _ in 0..runs {
        let mut totals = vec![0.0; parties.len()];
        let mut disc = 1.0;
        let mut cur = n;
        let mut ended = false;
        for _ in 0..MAX_RUN_STEPS {
            match v.plan[cur].control {
                Control::Terminal => {
                    for (t, p) in totals.iter_mut().zip(parties) {
                        *t += disc * p.terminal[cur];
                    }
                    ended = true;
                    break;
                }
                Control::Sink => {
                    ended = true;
                    break;
                }
                _ => {}
            }
            let dist = step_dist(v, choice, cur);
            let Some(e) = pick(&mut rng, &dist) else {
                ended = true;
                break;
            };
            for (t, p) in totals.iter_mut().zip(parties) {
                *t -= disc * p.cost[e];
            }
            disc *= gamma(v, e);
            cur = v.g.edges[e].to;
        }
        truncated += usize::from(!ended);
        for (xs, t) in samples.iter_mut().zip(totals) {
            xs.push(t);
        }
    }
    let out = samples
        .into_iter()
        .zip(parties)
        .map(|(xs, p)| match p.risk {
            Risk::Cvar(alpha) => tail_mean(xs, alpha),
            _ => xs.iter().sum::<f64>() / xs.len() as f64,
        })
        .collect();
    (out, truncated)
}

fn pick(rng: &mut ChaCha8Rng, dist: &[(usize, f64)]) -> Option<usize> {
    let total: f64 = dist.iter().map(|d| d.1).sum();
    let mut u = rng.gen::<f64>() * total;
    for &(e, p) in dist {
        if u < p {
            return Some(e);
        }
        u -= p;
    }
    dist.last().map(|d| d.0)
}

/// Mean of the worst `alpha` fraction of `xs`.
fn tail_mean(mut xs: Vec<f64>, alpha: f64) -> f64 {
    xs.sort_by(f64::total_cmp);
    // `alpha` is validated to (0, 1] and `xs` is non-empty, so the count is
    // in `[1, xs.len()]`.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let k = ((alpha * xs.len() as f64).ceil() as usize).clamp(1, xs.len());
    xs[..k].iter().sum::<f64>() / k as f64
}

/// The most likely line from `n` under the policy: non-terminal nodes only.
pub(crate) fn likely_line(
    v: &View,
    choice: &BTreeMap<NodeIx, usize>,
    n: NodeIx,
    max: usize,
) -> Vec<(NodeIx, f64)> {
    let mut out = vec![];
    let mut seen = vec![false; v.g.nodes.len()];
    let mut cur = n;
    let mut p_here = 1.0;
    while out.len() < max && !seen[cur] {
        seen[cur] = true;
        if matches!(v.plan[cur].control, Control::Terminal | Control::Sink) {
            break;
        }
        out.push((cur, p_here));
        let Some((e, p)) = step_dist(v, choice, cur)
            .into_iter()
            .max_by(|a, b| a.1.total_cmp(&b.1))
        else {
            break;
        };
        p_here = p;
        cur = v.g.edges[e].to;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_mean_takes_the_worst_fraction() {
        assert_eq!(tail_mean(vec![3.0, 1.0, 2.0, 4.0], 0.5), 1.5);
        assert_eq!(tail_mean(vec![5.0], 0.01), 5.0);
        assert_eq!(tail_mean(vec![1.0, 3.0], 1.0), 2.0);
    }

    #[test]
    fn pick_draws_by_weight_and_handles_an_empty_distribution() {
        let mut rng = ChaCha8Rng::seed_from_u64(1);
        assert_eq!(pick(&mut rng, &[]), None);
        assert_eq!(pick(&mut rng, &[(4, 0.0), (9, 1.0)]), Some(9));
    }

    #[test]
    fn risk_maps_objectives_and_sure_money_utility() {
        assert_eq!(Risk::of(&Objective::Worst), Risk::Worst);
        assert_eq!(Risk::of(&Objective::Cara { a: 0.1 }).cara(), 0.1);
        assert_eq!(Risk::Cvar(0.2).cara(), 0.0);
        assert_eq!(Risk::Cvar(0.2).recursive(), Risk::Expected);
        assert_eq!(Risk::Worst.aggregate(&[(0.5, 1.0), (0.5, 3.0)]), 1.0);
    }
}
