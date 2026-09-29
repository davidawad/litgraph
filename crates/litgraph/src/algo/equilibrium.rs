// SPDX-License-Identifier: GPL-3.0-or-later
//! General-sum opponents: a subgame-perfect equilibrium by backward
//! induction on the graph's SCC DAG.
//!
//! The graph is an extensive-form, perfect-information game: at every node
//! exactly one party (self, the opponent, or nature) moves. For such a game,
//! backward induction over the acyclic part of the graph computes the
//! (unique, for generic payoffs) subgame-perfect equilibrium directly: work
//! from terminals inward, and at each node the mover picks the edge that
//! maximizes *their own* continuation value, given every later node's
//! already-solved equilibrium.
//!
//! `self` uses [`crate::scenario::Scenario::utility`]/`cost` as always. The
//! opponent's payoff is [`crate::scenario::Scenario::opponent_objective`] (a
//! terminal expression) accumulated against the `opponent_dollars` built-in
//! metric per edge — symmetric to how `self`'s value is `utility` (terminal)
//! accumulated against `cost` (edges).
//!
//! Cyclic components can't be topologically ordered, so best-response
//! updates are iterated jointly (both players' values) to a fixed point,
//! exactly mirroring [`crate::algo::mdp`]'s cyclic handling — including its
//! honesty about non-convergence (best-response iteration in a general-sum
//! game is not guaranteed to converge the way single-agent value iteration
//! is; see `docs/CRITIQUE.md`).
//!
//! `opponent_objective: None` (the default) is the existing zero-sum
//! adversarial/chance opponent model, handled here by delegating to
//! [`crate::algo::mdp::solve`] unchanged — the special case the brief and
//! tests require to reproduce byte-for-byte.
//!
//! `self`'s risk objective (`Objective::Cara`/`Worst`) is honored: it's
//! applied, via the same [`mdp::aggregate`] helper `mdp::solve` uses, to
//! *self*'s aggregation over nature's draws — both when choosing whether to
//! wait ([`Ctx::best_option`]'s `crit_wait`) and in the final value tally
//! ([`Ctx::backup`]) — never to the opponent's, which stays plain
//! expectation: general-sum gives the opponent an objective function, not a
//! modeled risk preference. `Objective::Cvar` doesn't compose with a
//! general-sum opponent at all (see `docs/CRITIQUE.md`).

use serde::Serialize;
use std::collections::BTreeMap;

use crate::algo::mdp::{self, gamma, Solution, SolveOptions};
use crate::algo::structure::{is_cyclic, scc};
use crate::error::Result;
use crate::expr::{self, Expr};
use crate::metrics::{PathVars, TerminalEnv};
use crate::model::NodeIx;
use crate::scenario::{Control, Objective, View, WAIT};

/// Result of resolving `v`: `self`'s solution plus the opponent's values.
#[derive(Debug, Clone, Serialize)]
pub struct EquilibriumSolution {
    /// `self`'s solution (value, per-edge q, policy) — identical to
    /// `mdp::solve`'s output when `opponent_objective` is `None`.
    pub solution: Solution,
    /// The opponent's value at every node under the same policy.
    pub opponent_value: Vec<f64>,
    /// The opponent's own per-edge continuation value (`NaN` for inactive
    /// edges / edges out of terminals, same convention as `solution.q`).
    /// Zero-sum falls out as `-solution.q` (the identity used for
    /// `opponent_value`); general-sum uses their actual `opp_q`. This is
    /// what a general-sum opponent's own choice is optimal *for* — `explain`
    /// uses it to compute `regret` from the mover's own criterion instead of
    /// always assuming an adversary.
    pub opponent_q: Vec<f64>,
    /// `true` for a general-sum opponent (`opponent_objective` was set).
    pub general_sum: bool,
}

/// Resolves `v`: the existing zero-sum solve when `v.sc.opponent_objective`
/// is `None` (`opponent_value` reported as `-solution.value`, the zero-sum
/// identity), otherwise a general-sum subgame-perfect equilibrium.
///
/// # Errors
/// Propagates fee-eligibility/expression errors from `mdp::solve`, or an
/// error evaluating `opponent_objective` at a terminal.
pub fn resolve(v: &View, opts: &SolveOptions) -> Result<EquilibriumSolution> {
    let Some(obj) = v.sc.opponent_objective.as_deref() else {
        let solution = mdp::solve(v, opts)?;
        let opponent_value = solution.value.iter().map(|x| -x).collect();
        let opponent_q = solution.q.iter().map(|x| -x).collect();
        return Ok(EquilibriumSolution {
            solution,
            opponent_value,
            opponent_q,
            general_sum: false,
        });
    };
    resolve_general_sum(v, obj, opts)
}

fn resolve_general_sum(v: &View, obj: &str, opts: &SolveOptions) -> Result<EquilibriumSolution> {
    let opp_ex = expr::parse(obj)?;
    let opp_cost = v.metric("opponent_dollars")?;
    let opp_terminal: Vec<f64> = (0..v.g.nodes.len())
        .map(|n| {
            if !v.g.nodes[n].is_terminal() {
                return Ok(0.0);
            }
            eval_opponent(&opp_ex, v, n)
        })
        .collect::<Result<_>>()?;
    let cara = match v.sc.objective {
        Objective::Cara { a } => Some(a),
        _ => None,
    };
    let worst = v.sc.objective == Objective::Worst;
    let ctx = Ctx {
        v,
        cost: &v.cost,
        opp_cost: &opp_cost,
        opp_terminal: &opp_terminal,
        cara,
        worst,
    };
    let (self_v, opp_v, choice, unconverged, worst_it) = joint_backward_induction(&ctx, opts);
    let edge_q = |f: &dyn Fn(&Ctx, usize) -> f64| -> Vec<f64> {
        (0..v.g.edges.len())
            .map(|e| {
                if !v.active[e] || v.plan[v.g.edges[e].from].control == Control::Terminal {
                    f64::NAN
                } else {
                    f(&ctx, e)
                }
            })
            .collect()
    };
    let q = edge_q(&|ctx, e| ctx.self_q(e, &self_v));
    let opponent_q = edge_q(&|ctx, e| ctx.opp_q(e, &opp_v));
    let solution = Solution {
        value: self_v,
        q,
        choice,
        converged: unconverged.is_empty(),
        unconverged,
        iterations: worst_it,
        fee_shift_rounds: 0,
        cost: v.cost.clone(),
    };
    Ok(EquilibriumSolution {
        solution,
        opponent_value: opp_v,
        opponent_q,
        general_sum: true,
    })
}

/// One SCC-ordered joint backward-induction pass: `(self_value,
/// opponent_value, choice, unconverged, worst_iterations)`. Acyclic
/// components get one exact backup; cyclic components iterate both players'
/// values jointly to a fixed point, exactly mirroring `mdp::solve_with`.
#[allow(clippy::type_complexity)]
fn joint_backward_induction(
    ctx: &Ctx,
    opts: &SolveOptions,
) -> (
    Vec<f64>,
    Vec<f64>,
    BTreeMap<NodeIx, usize>,
    Vec<NodeIx>,
    usize,
) {
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
    let n = ctx.v.g.nodes.len();
    let mut self_v = vec![0.0; n];
    let mut opp_v = vec![0.0; n];
    let mut choice = BTreeMap::new();
    let mut unconverged = vec![];
    let mut worst_it = 0;
    for comp in scc(ctx.v) {
        if !is_cyclic(ctx.v, &comp) {
            let (s, o, c) = ctx.backup(comp[0], &self_v, &opp_v);
            self_v[comp[0]] = s;
            opp_v[comp[0]] = o;
            if let Some(c) = c {
                choice.insert(comp[0], c);
            }
            continue;
        }
        let mut it = 0;
        loop {
            it += 1;
            let mut delta: f64 = 0.0;
            for &u in &comp {
                let (s, o, _) = ctx.backup(u, &self_v, &opp_v);
                delta = delta.max((s - self_v[u]).abs()).max((o - opp_v[u]).abs());
                self_v[u] = s;
                opp_v[u] = o;
            }
            let scale = comp
                .iter()
                .map(|&u| self_v[u].abs().max(opp_v[u].abs()))
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
        for &u in &comp {
            if let (_, _, Some(c)) = ctx.backup(u, &self_v, &opp_v) {
                choice.insert(u, c);
            }
        }
    }
    (self_v, opp_v, choice, unconverged, worst_it)
}

fn eval_opponent(ex: &Expr, v: &View, n: NodeIx) -> Result<f64> {
    ex.eval(&TerminalEnv {
        g: v.g,
        n,
        payoff: v.payoff[n],
        params: &v.params,
        path: PathVars::default(),
    })
}

struct Ctx<'a> {
    v: &'a View<'a>,
    cost: &'a [f64],
    opp_cost: &'a [f64],
    opp_terminal: &'a [f64],
    /// `self`'s risk objective (`Objective::Cara`/`Worst`), applied only to
    /// *self*'s aggregation over nature's draws (world edges, interrupts,
    /// `WAIT`) — never to the opponent's own aggregation, which stays plain
    /// expectation: general-sum gives the opponent an objective function,
    /// not a modeled risk preference. See `docs/CRITIQUE.md`.
    cara: Option<f64>,
    worst: bool,
}

impl Ctx<'_> {
    fn self_q(&self, e: usize, value: &[f64]) -> f64 {
        -self.cost[e] + gamma(self.v, e) * value[self.v.g.edges[e].to]
    }

    fn opp_q(&self, e: usize, opp_value: &[f64]) -> f64 {
        -self.opp_cost[e] + gamma(self.v, e) * opp_value[self.v.g.edges[e].to]
    }

    /// The mover's chosen `(criterion-value, edge)` at node `n`: self
    /// maximizes `self_q`; a general-sum opponent maximizes their own
    /// `opp_q` (never `minimize`, unlike `mdp::Ctx`'s adversarial default —
    /// general-sum implies a self-interested chooser; see `plan.rs`). Ties
    /// go to the option nearer a terminal, exactly like `mdp::Ctx::backup`.
    fn best_option(&self, n: NodeIx, value: &[f64], opp_value: &[f64]) -> Option<(f64, usize)> {
        let p = &self.v.plan[n];
        let crit = |e: usize| -> f64 {
            if p.control == Control::Opponent {
                self.opp_q(e, opp_value)
            } else {
                self.self_q(e, value)
            }
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
        // Self compares its own risk-adjusted value of waiting against its
        // other options (matching the risk objective applied to the final
        // tally in `backup`); an opponent's `WAIT` is always plain
        // expectation, like every other opponent aggregation.
        let crit_wait = || -> Option<f64> {
            (!p.wait.is_empty()).then(|| {
                let terms: Vec<(f64, f64)> = p.wait.iter().map(|&(e, pr)| (pr, crit(e))).collect();
                if p.control == Control::Opponent {
                    terms.iter().map(|&(pr, q)| pr * q).sum()
                } else {
                    mdp::aggregate(self.cara, self.worst, &terms)
                }
            })
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
        if let Some(f) = forced {
            let c = if f == WAIT {
                crit_wait().unwrap_or(f64::NEG_INFINITY)
            } else {
                crit(f)
            };
            return Some((c, f));
        }
        let options = p.choices.iter().copied().chain(crit_wait().map(|_| WAIT));
        let mut best: Option<(f64, usize)> = None;
        for e in options {
            let c = if e == WAIT {
                crit_wait().unwrap_or(f64::NEG_INFINITY)
            } else {
                crit(e)
            };
            let better = match best {
                None => true,
                Some((b, be)) => {
                    let tol = 1e-9 * b.abs().max(1.0);
                    if (c - b).abs() <= tol {
                        dist(e) < dist(be)
                    } else {
                        c > b
                    }
                }
            };
            if better {
                best = Some((c, e));
            }
        }
        best
    }

    /// One joint backup at node `n`: `(self_value, opponent_value, choice)`.
    fn backup(&self, n: NodeIx, value: &[f64], opp_value: &[f64]) -> (f64, f64, Option<usize>) {
        let p = &self.v.plan[n];
        match p.control {
            Control::Terminal => return (self.v.utility[n], self.opp_terminal[n], None),
            Control::Sink => return (0.0, 0.0, None),
            _ => {}
        }
        let best = self.best_option(n, value, opp_value);
        // Self's risk objective (if any) applies to `WAIT`'s aggregation over
        // world edges exactly as it would in `mdp::Ctx` (a single-agent node
        // with an interrupt uses the same rule); the opponent's `WAIT`
        // aggregation is always plain expectation.
        let self_q_of = |e: usize| -> f64 {
            if e == WAIT {
                let terms: Vec<(f64, f64)> = p
                    .wait
                    .iter()
                    .map(|&(w, pr)| (pr, self.self_q(w, value)))
                    .collect();
                mdp::aggregate(self.cara, self.worst, &terms)
            } else {
                self.self_q(e, value)
            }
        };
        let opp_q_of = |e: usize| -> f64 {
            if e == WAIT {
                p.wait
                    .iter()
                    .map(|&(w, pr)| pr * self.opp_q(w, opp_value))
                    .sum()
            } else {
                self.opp_q(e, opp_value)
            }
        };
        let self_terms: Vec<(f64, f64)> = p
            .draws
            .iter()
            .map(|&(e, pr)| (pr, self.self_q(e, value)))
            .chain(
                best.filter(|_| p.choice_mass > 0.0)
                    .map(|(_, e)| (p.choice_mass, self_q_of(e))),
            )
            .collect();
        let opp_draws: f64 = p
            .draws
            .iter()
            .map(|&(e, pr)| pr * self.opp_q(e, opp_value))
            .sum();
        let opp_choice_part = best
            .filter(|_| p.choice_mass > 0.0)
            .map_or(0.0, |(_, e)| opp_q_of(e));
        let self_val = mdp::aggregate(self.cara, self.worst, &self_terms);
        let opp_val = opp_draws + p.choice_mass * opp_choice_part;
        (self_val, opp_val, best.map(|(_, e)| e))
    }
}
