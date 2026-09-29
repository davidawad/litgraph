// SPDX-License-Identifier: GPL-3.0-or-later
//! Solve the procedure as a stochastic game.
//!
//! Each node's plan (scenario.rs) says who chooses and what may interrupt:
//!
//! ```text
//! q(e)  = −cost(e) + γ(e)·V(e.to)                 γ(e) = (1+r)^(−elapsed/365)
//! V(n)  = Σ_draws p·q(e) + mass · best_{choices} q(e)     (best = max, or min for an adversary)
//! V(t)  = utility(t) at terminals
//! ```
//!
//! With a CARA objective (risk aversion a), expectations are replaced by
//! certainty equivalents `−(1/a)·ln Σ p·exp(−a·q)`, which is exact for
//! exponential utility because it is translation-invariant.
//!
//! Nodes are solved in reverse topological order of the SCC condensation:
//! acyclic parts get one exact backup, cycles (RCE loops, discovery
//! self-loops) are iterated to convergence locally. This is both faster and
//! more exact than v1's whole-graph sweeps.
//!
//! Fee shifting is exact for a fixed policy: our cost on an edge is recovered
//! in proportion to the probability of ending at a fee-eligible terminal from
//! that edge's target, `c'(e) = c(e)·(1 − f·P_elig(e.to))`. Because
//! P_elig depends on the policy, we alternate solve ↔ evaluate until the
//! policy stops changing (policy iteration).

use serde::Serialize;
use std::collections::BTreeMap;

use crate::algo::structure::{is_cyclic, scc};
use crate::error::Result;
use crate::expr;
use crate::metrics::TerminalEnv;
use crate::model::{NodeIx, Role};
use crate::scenario::{Control, Objective, View, WAIT};

/// Tolerances for the cyclic (SCC-local) value-iteration backup.
#[derive(Debug, Clone, Default)]
pub struct SolveOptions {
    /// Convergence tolerance, relative to the component's value scale (0 = default `1e-9`).
    pub epsilon: f64,
    /// Iteration cap per cyclic component before giving up (0 = default `100_000`).
    pub max_iterations: usize,
}

/// The solved game: values, action values, and the optimal policy.
#[derive(Debug, Clone, Serialize)]
pub struct Solution {
    /// Value of every node under the optimal (or fixed-role) policy.
    pub value: Vec<f64>,
    /// Per-edge action value (NaN for inactive edges / edges out of terminals).
    pub q: Vec<f64>,
    /// Chosen edge at every node where a chooser (self or opponent) acts.
    pub choice: BTreeMap<NodeIx, usize>,
    /// True if every cyclic component converged within its iteration cap.
    pub converged: bool,
    /// Nodes in cycles that hit the iteration cap (values unreliable; usually a
    /// loop someone can force forever at positive cost).
    pub unconverged: Vec<NodeIx>,
    /// Iterations used by the slowest-converging cyclic component.
    pub iterations: usize,
    /// Fee-shift policy-iteration rounds used (0 if no `fee_shift`).
    pub fee_shift_rounds: usize,
    /// Effective (possibly fee-shift-adjusted) edge costs used.
    pub cost: Vec<f64>,
}

impl Solution {
    /// Value of option `e` at node `n` (the expectation over the world edges when `e` is WAIT).
    #[must_use]
    pub fn option_q(&self, v: &View, n: NodeIx, e: usize) -> f64 {
        if e == WAIT {
            v.plan[n].wait.iter().map(|&(w, p)| p * self.q[w]).sum()
        } else {
            self.q[e]
        }
    }

    /// Our (role self) chosen edges only.
    pub fn my_policy<'a>(&'a self, v: &'a View) -> impl Iterator<Item = (NodeIx, usize)> + 'a {
        self.choice
            .iter()
            .filter(move |(n, _)| v.plan[**n].control == Control::Me)
            .map(|(n, e)| (*n, *e))
    }
}

fn gamma(v: &View, e: usize) -> f64 {
    match v.sc.discount_annual {
        Some(r) if r != 0.0 => (1.0 + r).powf(-v.elapsed[e] / 365.0),
        _ => 1.0,
    }
}

struct Ctx<'a> {
    v: &'a View<'a>,
    cost: &'a [f64],
    cara: Option<f64>,
    /// `Objective::Worst`: every draw (interrupts, waits) goes against us.
    worst: bool,
}

impl Ctx<'_> {
    fn q(&self, e: usize, value: &[f64]) -> f64 {
        -self.cost[e] + gamma(self.v, e) * value[self.v.g.edges[e].to]
    }

    /// One backup at node n. Returns (value, chosen edge).
    fn backup(&self, n: NodeIx, value: &[f64]) -> (f64, Option<usize>) {
        let p = &self.v.plan[n];
        match p.control {
            Control::Terminal => return (self.v.utility[n], None),
            Control::Sink => return (0.0, None),
            _ => {}
        }
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
        // WAIT = let the world edges fire (act-or-wait nodes).
        let q_wait = (!p.wait.is_empty()).then(|| {
            self.aggregate(
                &p.wait
                    .iter()
                    .map(|&(e, pr)| (pr, self.q(e, value)))
                    .collect::<Vec<_>>(),
            )
        });
        let q_of = |e: usize| {
            if e == WAIT {
                q_wait.unwrap_or(f64::NEG_INFINITY)
            } else {
                self.q(e, value)
            }
        };
        let mut best: Option<(f64, usize)> = None;
        if let Some(f) = forced {
            best = Some((q_of(f), f));
        } else {
            // Ties (within tolerance) go to the option whose target is fewest
            // steps from a terminal, so a zero-cost self-loop can never tie
            // its way into a policy that loops forever.
            let dist = |e: usize| {
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
            let options = p.choices.iter().copied().chain(q_wait.map(|_| WAIT));
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
        let terms: Vec<(f64, f64)> = p
            .draws
            .iter()
            .map(|&(e, pr)| (pr, self.q(e, value)))
            .chain(
                best.filter(|_| p.choice_mass > 0.0)
                    .map(|(q, _)| (p.choice_mass, q)),
            )
            .collect();
        (self.aggregate(&terms), best.map(|(_, e)| e))
    }

    /// Expectation, or the CARA certainty equivalent, of (probability, value) terms.
    fn aggregate(&self, terms: &[(f64, f64)]) -> f64 {
        if self.worst {
            return terms
                .iter()
                .filter(|t| t.0 > 0.0)
                .map(|t| t.1)
                .fold(f64::INFINITY, f64::min);
        }
        match self.cara {
            Some(a) if a != 0.0 => {
                // Certainty equivalent, computed stably around the max.
                let m = terms
                    .iter()
                    .map(|&(_, q)| -a * q)
                    .fold(f64::NEG_INFINITY, f64::max);
                let s: f64 = terms.iter().map(|&(pr, q)| pr * (-a * q - m).exp()).sum();
                -(m + s.ln()) / a
            }
            _ => terms.iter().map(|&(pr, q)| pr * q).sum(),
        }
    }
}

/// Solve with a fixed cost vector.
fn solve_with(
    v: &View,
    cost: &[f64],
    opts: &SolveOptions,
) -> (Vec<f64>, BTreeMap<NodeIx, usize>, Vec<NodeIx>, usize) {
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
    let cara = match v.sc.objective {
        Objective::Cara { a } => Some(a),
        _ => None,
    };
    let ctx = Ctx {
        v,
        cost,
        cara,
        worst: v.sc.objective == Objective::Worst,
    };
    let n = v.g.nodes.len();
    let mut value = vec![0.0; n];
    let mut choice = BTreeMap::new();
    let mut unconverged = vec![];
    let mut worst_it = 0;
    for comp in scc(v) {
        if !is_cyclic(v, &comp) {
            let (val, c) = ctx.backup(comp[0], &value);
            value[comp[0]] = val;
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
                let (val, _) = ctx.backup(u, &value);
                delta = delta.max((val - value[u]).abs());
                value[u] = val;
            }
            // Relative tolerance: values are in dollars.
            let scale = comp.iter().map(|&u| value[u].abs()).fold(1.0, f64::max);
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
            if let (_, Some(c)) = ctx.backup(u, &value) {
                choice.insert(u, c);
            }
        }
    }
    (value, choice, unconverged, worst_it)
}

/// Probability of absorbing in a node with `target[n] = 1` from every node,
/// under a fixed choice map (draws per plan, choices deterministic).
#[must_use]
pub fn absorb_prob(v: &View, choice: &BTreeMap<NodeIx, usize>, target: &[f64]) -> Vec<f64> {
    let n = v.g.nodes.len();
    let mut p = vec![0.0; n];
    for comp in scc(v) {
        let cyclic = is_cyclic(v, &comp);
        let mut it = 0;
        loop {
            it += 1;
            let mut delta: f64 = 0.0;
            for &u in &comp {
                let plan = &v.plan[u];
                let new = match plan.control {
                    Control::Terminal => target[u],
                    Control::Sink => 0.0,
                    _ => {
                        let mut acc: f64 = plan
                            .draws
                            .iter()
                            .map(|&(e, pr)| pr * p[v.g.edges[e].to])
                            .sum();
                        if plan.choice_mass > 0.0 {
                            if choice.get(&u) == Some(&WAIT) {
                                acc += plan.choice_mass
                                    * plan
                                        .wait
                                        .iter()
                                        .map(|&(e, pr)| pr * p[v.g.edges[e].to])
                                        .sum::<f64>();
                            } else if let Some(&e) = choice.get(&u) {
                                acc += plan.choice_mass * p[v.g.edges[e].to];
                            } else {
                                // Worst-case nature with no recorded choice:
                                // average (`max(1)` guards the no-choice case).
                                let k = (plan.choices.len() as f64).max(1.0);
                                acc += plan.choice_mass
                                    * plan
                                        .choices
                                        .iter()
                                        .map(|&e| p[v.g.edges[e].to])
                                        .sum::<f64>()
                                    / k;
                            }
                        }
                        acc
                    }
                };
                delta = delta.max((new - p[u]).abs());
                p[u] = new;
            }
            if !cyclic || delta < 1e-12 || it > 100_000 {
                break;
            }
        }
    }
    p
}

/// Solves the stochastic game over `v`: values, optimal policy, and (if
/// `v.sc.fee_shift` is set) the fee-shift-adjusted costs it converged on.
///
/// ```
/// use litgraph::algo::mdp;
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
/// // One free edge to a $100 terminal: the start is worth exactly $100.
/// assert_eq!(sol.value[v.start], 100.0);
/// ```
///
/// # Errors
/// Propagates any error evaluating the fee-eligibility expression.
pub fn solve(v: &View, opts: &SolveOptions) -> Result<Solution> {
    let base = v.cost.clone();
    let Some(fs) = v.sc.fee_shift.clone() else {
        let (value, choice, unconverged, iterations) = solve_with(v, &base, opts);
        let q = q_values(v, &base, &value);
        return Ok(Solution {
            value,
            q,
            choice,
            converged: unconverged.is_empty(),
            unconverged,
            iterations,
            fee_shift_rounds: 0,
            cost: base,
        });
    };
    let elig_src = fs
        .eligible
        .clone()
        .unwrap_or_else(|| "tag('fee-eligible')".into());
    let elig_expr = expr::parse(&elig_src)?;
    let elig: Vec<f64> = (0..v.g.nodes.len())
        .map(|n| {
            if !v.g.nodes[n].is_terminal() {
                return Ok(0.0);
            }
            let x = elig_expr.eval(&TerminalEnv {
                g: v.g,
                n,
                payoff: v.payoff[n],
                params: &v.params,
            })?;
            Ok(if x == 0.0 { 0.0 } else { 1.0 })
        })
        .collect::<Result<_>>()?;
    let mut cost = base.clone();
    let mut prev: Option<BTreeMap<NodeIx, usize>> = None;
    let mut rounds = 0;
    loop {
        rounds += 1;
        let (value, choice, unconverged, iterations) = solve_with(v, &cost, opts);
        let stable = prev.as_ref() == Some(&choice);
        if stable || rounds >= 25 {
            let q = q_values(v, &cost, &value);
            return Ok(Solution {
                value,
                q,
                choice,
                converged: unconverged.is_empty() && stable,
                unconverged,
                iterations,
                fee_shift_rounds: rounds,
                cost,
            });
        }
        let pe = absorb_prob(v, &choice, &elig);
        cost = (0..base.len())
            .map(|e| {
                if v.role[e] == Role::Me && v.active[e] {
                    base[e] * (1.0 - fs.fraction * pe[v.g.edges[e].to])
                } else {
                    base[e]
                }
            })
            .collect();
        prev = Some(choice);
    }
}

fn q_values(v: &View, cost: &[f64], value: &[f64]) -> Vec<f64> {
    (0..v.g.edges.len())
        .map(|e| {
            if !v.active[e] || v.plan[v.g.edges[e].from].control == Control::Terminal {
                f64::NAN
            } else {
                -cost[e] + gamma(v, e) * value[v.g.edges[e].to]
            }
        })
        .collect()
}
