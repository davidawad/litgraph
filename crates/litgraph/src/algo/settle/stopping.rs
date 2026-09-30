// SPDX-License-Identifier: GPL-3.0-or-later
//! Settling as an always-available action: optimal stopping for one side.
//!
//! At every node the side may settle at the predicted price (when a deal
//! zone exists there) or continue. Continuing means the usual game step,
//! except that the side re-optimizes its own choices knowing it can settle
//! later; the other side's choices and nature's draws stay as solved.
//!
//! ```text
//! W(n) = max( settle(n), cont(n) )
//! cont(n) = risk-aggregate over the step from n of  −cost(e) + γ(e)·W(e.to)
//! ```

use std::collections::BTreeMap;

use super::value::{fixed_point, Party};
use crate::algo::chain::step_dist;
use crate::algo::mdp::gamma;
use crate::model::NodeIx;
use crate::scenario::{Control, View, WAIT};

/// One side's stopping problem, solved.
#[derive(Debug, Clone)]
pub(crate) struct Stopping {
    /// Value with the settlement option.
    pub value: Vec<f64>,
    /// Value of continuing one more step (keeping the option afterwards).
    pub cont: Vec<f64>,
    /// Settling now strictly beats continuing.
    pub stop: Vec<bool>,
    /// The side's own choice when it continues, where it moves.
    pub choice: BTreeMap<NodeIx, usize>,
    /// Every cyclic component converged.
    pub converged: bool,
}

/// Solves the stopping problem for the side moving at `mover` nodes, with
/// `settle[n]` its value of settling at `n` (`-inf` where there is no deal).
pub(crate) fn solve(
    v: &View,
    choice: &BTreeMap<NodeIx, usize>,
    party: &Party,
    mover: Control,
    settle: &[f64],
) -> Stopping {
    let risk = party.risk.recursive();
    let q = |e: usize, w: &[f64]| -party.cost[e] + gamma(v, e) * w[v.g.edges[e].to];
    let cont_of = |n: NodeIx, w: &[f64]| -> (f64, Option<usize>) {
        let plan = &v.plan[n];
        match plan.control {
            Control::Terminal => return (party.terminal[n], None),
            Control::Sink => return (0.0, None),
            c if c != mover => {
                let terms: Vec<(f64, f64)> = step_dist(v, choice, n)
                    .into_iter()
                    .map(|(e, p)| (p, q(e, w)))
                    .collect();
                return (risk.aggregate(&terms), None);
            }
            _ => {}
        }
        let q_wait = (!plan.wait.is_empty()).then(|| {
            let terms: Vec<(f64, f64)> = plan.wait.iter().map(|&(e, p)| (p, q(e, w))).collect();
            risk.aggregate(&terms)
        });
        let q_opt = |e: usize| {
            if e == WAIT {
                q_wait.unwrap_or(f64::NEG_INFINITY)
            } else {
                q(e, w)
            }
        };
        let forced = (mover == Control::Me)
            .then(|| v.forced.get(&n).copied())
            .flatten()
            .map(|f| {
                if plan.wait.iter().any(|&(e, _)| e == f) {
                    WAIT
                } else {
                    f
                }
            });
        let best = match forced {
            Some(f) => Some((q_opt(f), f)),
            None => plan
                .choices
                .iter()
                .copied()
                .chain(q_wait.map(|_| WAIT))
                .map(|e| (q_opt(e), e))
                .fold(None, |best: Option<(f64, usize)>, (x, e)| match best {
                    Some((b, _)) if x <= b + 1e-9 * b.abs().max(1.0) => best,
                    _ => Some((x, e)),
                }),
        };
        let terms: Vec<(f64, f64)> = plan
            .draws
            .iter()
            .map(|&(e, p)| (p, q(e, w)))
            .chain(
                best.filter(|_| plan.choice_mass > 0.0)
                    .map(|(x, _)| (plan.choice_mass, x)),
            )
            .collect();
        (risk.aggregate(&terms), best.map(|b| b.1))
    };
    let (value, converged) = fixed_point(v, |n, w| cont_of(n, w).0.max(settle[n]));
    let mut cont = vec![0.0; value.len()];
    let mut stop = vec![false; value.len()];
    let mut own = BTreeMap::new();
    for n in 0..value.len() {
        let (c, e) = cont_of(n, &value);
        cont[n] = c;
        stop[n] = !v.g.nodes[n].is_terminal() && settle[n] > c + 1e-9 * c.abs().max(1.0);
        if let Some(e) = e {
            own.insert(n, e);
        }
    }
    Stopping {
        value,
        cont,
        stop,
        choice: own,
        converged,
    }
}
