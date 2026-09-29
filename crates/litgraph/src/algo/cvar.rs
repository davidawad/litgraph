// SPDX-License-Identifier: GPL-3.0-or-later
//! `CVaR`-optimal policies via Rockafellar–Uryasev.
//!
//! `CVaR_alpha(X)`, the mean of the worst `alpha` fraction of the outcome
//! `X`, has the variational form (Rockafellar & Uryasev 2000):
//!
//! ```text
//! CVaR_alpha(X) = max_ζ [ ζ − (1/alpha)·E[(ζ − X)⁺] ]
//! ```
//!
//! concave in the threshold `ζ` (the Value-at-Risk). For a fixed `ζ`, the
//! inner expectation is itself an MDP objective once the state is augmented
//! with the "remaining budget" `y = ζ − (value accumulated so far)`: at a
//! terminal, the local objective `−(1/alpha)·max(y − u(terminal), 0)` is a
//! function of `y` alone, and taking an edge of value `−cost(e)` shifts the
//! budget by `y ↦ y + cost(e)` (Bäuerle & Ott 2011). Because that shift
//! doesn't depend on `ζ`, one backward-induction pass over a grid of `y`
//! values computes the optimal augmented value `W(node, y)` for *every*
//! candidate `ζ` at once — the outer maximization is then a lookup
//! `max_j [ ys[j] + W(start, ys[j]) ]` at the start node. See
//! `docs/CRITIQUE.md` ("CVaR-optimal policies") for exactness limits.

use serde::Serialize;
use std::collections::BTreeMap;

use crate::algo::mdp::{Solution, SolveOptions};
use crate::algo::structure::{is_cyclic, scc};
use crate::error::{Error, Result};
use crate::model::NodeIx;
use crate::scenario::{Control, View, WAIT};

/// Result of a `CVaR`-optimal solve.
#[derive(Debug, Clone, Serialize)]
pub struct CvarSolution {
    /// A `Solution`-shaped view of the result: `value[n]` is the achieved
    /// `CVaR_alpha`-to-go from `n` along the canonical rollout from the start
    /// (see [`solve`]'s docs); `choice` is the policy at the budget each node
    /// was first visited with.
    pub solution: Solution,
    /// Achieved `CVaR_alpha` of the total outcome from the start node.
    pub cvar: f64,
    /// The optimal Value-at-Risk threshold `ζ` found.
    pub zeta: f64,
    /// Tail fraction used.
    pub alpha: f64,
    /// Grid points used.
    pub grid: usize,
    /// The `(lo, hi)` outcome range the grid spans.
    pub y_range: (f64, f64),
}

fn linspace(lo: f64, hi: f64, n: usize) -> Vec<f64> {
    if n <= 1 {
        return vec![lo];
    }
    let step = (hi - lo) / (n - 1) as f64;
    (0..n).map(|i| lo + step * i as f64).collect()
}

/// Linear interpolation of `row` (values at `ys`) at `y`, clamped to the grid.
fn interp(row: &[f64], ys: &[f64], y: f64) -> f64 {
    if y <= ys[0] {
        return row[0];
    }
    let last = ys.len() - 1;
    if y >= ys[last] {
        return row[last];
    }
    // `partition_point` finds the first index whose grid value exceeds `y`;
    // `y` is strictly inside the grid here, so `i` is in `[1, last]`.
    let i = ys.partition_point(|&x| x <= y);
    let (lo, hi) = (ys[i - 1], ys[i]);
    let t = if hi > lo { (y - lo) / (hi - lo) } else { 0.0 };
    row[i - 1] + t * (row[i] - row[i - 1])
}

/// A conservative `(lo, hi)` bound on any single-simple-path outcome: the
/// authored terminal-utility range widened by the total absolute cost in the
/// graph. Costly cycles can exceed this bound (see `docs/CRITIQUE.md`); it
/// is a practical default, not a proof.
fn default_range(v: &View, cost: &[f64]) -> (f64, f64) {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for t in v.g.terminals() {
        lo = lo.min(v.utility[t]);
        hi = hi.max(v.utility[t]);
    }
    if !lo.is_finite() {
        lo = 0.0;
        hi = 0.0;
    }
    let total_cost: f64 = (0..v.g.edges.len())
        .filter(|&e| v.active[e] && cost[e].is_finite())
        .map(|e| cost[e].abs())
        .sum();
    (lo - total_cost - 1.0, hi + total_cost + 1.0)
}

struct Ctx<'a> {
    v: &'a View<'a>,
    cost: &'a [f64],
    alpha: f64,
    ys: &'a [f64],
}

impl Ctx<'_> {
    /// `W(to(e), y + cost(e))`, interpolated on the grid.
    fn q_row(&self, e: usize, w: &[Vec<f64>], y: f64) -> f64 {
        interp(&w[self.v.g.edges[e].to], self.ys, y + self.cost[e])
    }

    fn terminal_row(&self, n: NodeIx) -> Vec<f64> {
        let u = self.v.utility[n];
        self.ys
            .iter()
            .map(|&y| -(y - u).max(0.0) / self.alpha)
            .collect()
    }

    /// One backup at node `n` for every grid point `y`. Returns the row and,
    /// for a chooser, the chosen edge at every grid point (for policy
    /// reconstruction).
    fn backup(&self, n: NodeIx, w: &[Vec<f64>]) -> (Vec<f64>, Vec<Option<usize>>) {
        let p = &self.v.plan[n];
        match p.control {
            Control::Terminal => return (self.terminal_row(n), vec![None; self.ys.len()]),
            Control::Sink => return (vec![0.0; self.ys.len()], vec![None; self.ys.len()]),
            _ => {}
        }
        let q_wait = |y: f64| -> Option<f64> {
            (!p.wait.is_empty())
                .then(|| p.wait.iter().map(|&(e, pr)| pr * self.q_row(e, w, y)).sum())
        };
        let forced = if p.control == Control::Me {
            self.v.forced.get(&n).map(|&f| {
                if p.wait.iter().any(|&(e, _)| e == f) {
                    WAIT
                } else {
                    f
                }
            })
        } else {
            None
        };
        let dist = |e: usize| -> u32 {
            if e == WAIT {
                p.wait
                    .iter()
                    .map(|&(w, _)| self.v.dist_to_terminal[self.v.g.edges[w].to])
                    .min()
                    .unwrap_or(u32::MAX)
            } else {
                self.v.dist_to_terminal[self.v.g.edges[e].to]
            }
        };
        let mut row = vec![0.0; self.ys.len()];
        let mut chosen = vec![None; self.ys.len()];
        for (j, &y) in self.ys.iter().enumerate() {
            let q_of = |e: usize| -> f64 {
                if e == WAIT {
                    q_wait(y).unwrap_or(f64::NEG_INFINITY)
                } else {
                    self.q_row(e, w, y)
                }
            };
            let mut best: Option<(f64, usize)> = None;
            if let Some(f) = forced {
                best = Some((q_of(f), f));
            } else {
                let options = p.choices.iter().copied().chain(q_wait(y).map(|_| WAIT));
                for e in options {
                    let q = q_of(e);
                    let better = match best {
                        None => true,
                        Some((b, be)) => {
                            let tol = 1e-9 * b.abs().max(1.0);
                            if (q - b).abs() <= tol {
                                dist(e) < dist(be)
                            } else if p.minimize {
                                q < b
                            } else {
                                q > b
                            }
                        }
                    };
                    if better {
                        best = Some((q, e));
                    }
                }
            }
            let draws: f64 = p
                .draws
                .iter()
                .map(|&(e, pr)| pr * self.q_row(e, w, y))
                .sum();
            let choice_part = best.filter(|_| p.choice_mass > 0.0).map_or(0.0, |(q, _)| q);
            row[j] = draws + p.choice_mass * choice_part;
            chosen[j] = best.map(|(_, e)| e);
        }
        (row, chosen)
    }
}

/// Solves for a `CVaR_alpha`-optimal policy from `v.start`.
///
/// # Errors
/// `alpha` out of `(0, 1]`, or an error evaluating an expression the view
/// depends on (propagated from view construction of `v.utility`/`v.cost`,
/// which are already resolved by the time this is called).
#[allow(clippy::too_many_lines)]
pub fn solve(v: &View, alpha: f64, grid: usize, opts: &SolveOptions) -> Result<CvarSolution> {
    if !(alpha > 0.0 && alpha <= 1.0) {
        return Err(Error::Invalid(format!(
            "cvar objective: alpha must be in (0, 1], got {alpha}"
        )));
    }
    let n_grid = if grid == 0 { 41 } else { grid.max(2) };
    let cost = &v.cost;
    let y_range = default_range(v, cost);
    let ys = linspace(y_range.0, y_range.1, n_grid);
    let ctx = Ctx {
        v,
        cost,
        alpha,
        ys: &ys,
    };
    let eps = if opts.epsilon > 0.0 {
        opts.epsilon
    } else {
        1e-9
    };
    let max_it = if opts.max_iterations > 0 {
        opts.max_iterations
    } else {
        100_000
    };
    let n_nodes = v.g.nodes.len();
    let mut w: Vec<Vec<f64>> = vec![vec![0.0; n_grid]; n_nodes];
    let mut chosen: Vec<Vec<Option<usize>>> = vec![vec![None; n_grid]; n_nodes];
    let mut unconverged = vec![];
    let mut worst_it = 0;
    for comp in scc(v) {
        if !is_cyclic(v, &comp) {
            let (row, ch) = ctx.backup(comp[0], &w);
            w[comp[0]] = row;
            chosen[comp[0]] = ch;
            continue;
        }
        let mut it = 0;
        loop {
            it += 1;
            let mut delta: f64 = 0.0;
            for &u in &comp {
                let (row, ch) = ctx.backup(u, &w);
                for j in 0..n_grid {
                    delta = delta.max((row[j] - w[u][j]).abs());
                }
                w[u] = row;
                chosen[u] = ch;
            }
            let scale = comp
                .iter()
                .flat_map(|&u| w[u].iter().map(|x| x.abs()))
                .fold(1.0, f64::max);
            if delta <= eps * scale {
                break;
            }
            if it >= max_it {
                unconverged.extend(comp.iter().copied());
                break;
            }
        }
        worst_it = worst_it.max(it);
    }
    let start_row = &w[v.start];
    let (mut best_j, mut best_g) = (0, f64::NEG_INFINITY);
    for (j, &y) in ys.iter().enumerate() {
        let g = y + start_row[j];
        if g > best_g {
            best_g = g;
            best_j = j;
        }
    }
    let zeta = ys[best_j];
    // Reconstruct a canonical rollout from (start, zeta), recording the
    // first-visit choice and per-node achieved "CVaR-to-go" (y + W(n, y)).
    let mut value = vec![0.0; n_nodes];
    let mut choice = BTreeMap::new();
    let mut seen = vec![false; n_nodes];
    let mut cur = v.start;
    let mut y = zeta;
    let mut guard = 0;
    while !seen[cur] && guard <= n_nodes * n_grid + 1 {
        seen[cur] = true;
        value[cur] = y + interp(&w[cur], &ys, y);
        guard += 1;
        let j = ys.partition_point(|&x| x <= y).min(n_grid - 1);
        let Some(e) = chosen[cur][j] else { break };
        if v.plan[cur].control == Control::Me || v.plan[cur].control == Control::Opponent {
            choice.insert(cur, e);
        }
        if e == WAIT {
            // Deterministic continuation isn't defined for a draw; stop the
            // canonical rollout here (the chain/simulate ops still use the
            // real stochastic dynamics under this policy).
            break;
        }
        y += cost[e];
        cur = v.g.edges[e].to;
    }
    // Per-edge `q` is not given a single well-defined value under a
    // budget-dependent policy (the same edge's continuation value differs by
    // how much budget remains when it's taken); left `NaN` (rendered `null`)
    // rather than reporting a number that would depend on an arbitrary
    // choice of `y`. `value`/`choice` above (the canonical rollout from the
    // optimal `zeta`) are the CVaR-specific results.
    let q = vec![f64::NAN; v.g.edges.len()];
    let solution = Solution {
        value,
        q,
        choice,
        converged: unconverged.is_empty(),
        unconverged,
        iterations: worst_it,
        fee_shift_rounds: 0,
        cost: cost.clone(),
    };
    Ok(CvarSolution {
        solution,
        cvar: best_g,
        zeta,
        alpha,
        grid: n_grid,
        y_range,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linspace_endpoints() {
        let xs = linspace(0.0, 10.0, 5);
        assert_eq!(xs, vec![0.0, 2.5, 5.0, 7.5, 10.0]);
        assert_eq!(linspace(1.0, 2.0, 1), vec![1.0]);
    }

    #[test]
    fn interp_clamps_and_interpolates() {
        let ys = [0.0, 1.0, 2.0];
        let row = [0.0, 10.0, 20.0];
        assert!((interp(&row, &ys, -5.0) - 0.0).abs() < 1e-12);
        assert!((interp(&row, &ys, 5.0) - 20.0).abs() < 1e-12);
        assert!((interp(&row, &ys, 0.5) - 5.0).abs() < 1e-12);
    }
}
