// SPDX-License-Identifier: GPL-3.0-or-later
//! Settlement prediction from the game the engine already solves.
//!
//! From a node, each side's walk-away point is its certainty equivalent of
//! litigating on under the solved (general-sum) equilibrium policy: its own
//! terminal payoffs, its own costs, its own risk attitude. When the
//! plaintiff's walk-away price is below the defendant's, the gap is the zone
//! of possible agreement (ZOPA); [`bargain`] predicts a price inside it with
//! the standard models. Along the most likely line the range is recomputed
//! at every node (when settlement surplus peaks), and settling is treated as
//! an always-available action to show how the policy changes (optimal
//! stopping, [`stopping`]). An optional Rule 68 offer of judgment shifts
//! post-offer costs onto a plaintiff whose judgment is not more favorable
//! ([`rule68`]). See `docs/SETTLEMENT.md`.

pub mod bargain;
pub mod rule68;
mod stopping;
pub mod value;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::algo::equilibrium;
use crate::algo::mdp::SolveOptions;
use crate::error::{Error, Result};
use crate::expr;
use crate::metrics::{PathVars, TerminalEnv};
use crate::model::NodeIx;
use crate::scenario::{Control, Objective, View};
use bargain::{nash_price, round_delta, rubinstein, Patience, Rubinstein, Zopa};
pub use rule68::{Rule68, Rule68Outcome};
use value::{cvar_at, evaluate, likely_line, round_days, Party, Risk, Sampling};

/// Which side of the case `self` is on. Prices are what the defendant pays.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// `self` is paid (the claimant).
    #[default]
    Plaintiff,
    /// `self` pays (the party defending against the claim).
    Defendant,
}

/// An optional number per side of the case.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Sides {
    /// The plaintiff's value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plaintiff: Option<f64>,
    /// The defendant's value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defendant: Option<f64>,
}

/// Everything `settle` takes besides the scenario.
#[derive(Debug, Clone, PartialEq)]
pub struct SettleOptions {
    /// `self`'s side.
    pub self_side: Side,
    /// The opponent's risk attitude (`self`'s is `scenario.objective`).
    pub opponent_risk: Objective,
    /// The plaintiff's Nash bargaining power in `[0, 1]`.
    pub bargaining_power: f64,
    /// Annual discount rates for bargaining patience (default:
    /// `scenario.discount_annual`, else none).
    pub discount_annual: Sides,
    /// Per-round discount factors in `(0, 1]`, overriding the rates.
    pub delta: Sides,
    /// An offer of judgment to evaluate.
    pub rule68: Option<Rule68>,
    /// Monte Carlo budget for `CVaR` sides.
    pub sampling: Sampling,
    /// Length cap on the timing line.
    pub max_steps: usize,
}

/// Predicted prices inside a deal zone.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Prices {
    /// Split-the-surplus midpoint.
    pub midpoint: f64,
    /// Symmetric Nash bargaining solution (power 1/2, in each side's utility).
    pub nash_symmetric: f64,
    /// Weighted Nash bargaining solution at the plaintiff's `bargaining_power`.
    pub nash_weighted: f64,
    /// Rubinstein alternating offers.
    pub rubinstein: Rubinstein,
}

/// One node of the timing line.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct LinePoint {
    /// The node.
    pub node: NodeIx,
    /// Probability of the step that led here along the likely line.
    pub step_p: f64,
    /// The range at this node.
    pub zopa: Zopa,
    /// Weighted Nash price here, if a deal zone exists.
    pub price: Option<f64>,
    /// `self`'s value of continuing (with the option to settle later).
    pub self_continue: f64,
    /// `self` strictly prefers settling here to continuing.
    pub self_settles: bool,
    /// The opponent strictly prefers settling here to continuing.
    pub opponent_settles: bool,
}

/// When to settle: the range along the likely line and the stopping policy.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Timing {
    /// The likely line from the evaluated node.
    pub line: Vec<LinePoint>,
    /// Index into `line` where the surplus peaks (none if never positive).
    pub peak: Option<usize>,
    /// Index into `line` of the first node where `self` settles.
    pub first_settle: Option<usize>,
    /// `self`'s value litigating to the end (no settlement option).
    pub value_litigate: f64,
    /// `self`'s value with settlement always available.
    pub value_with_settlement: f64,
    /// Line nodes where `self`'s move changes once settling is available.
    pub changes: Vec<Change>,
}

/// A line node where the settlement option changes what `self` does.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Change {
    /// The node.
    pub node: NodeIx,
    /// `self`'s choice without the option (`None` where `self` doesn't move).
    pub litigation: Option<usize>,
    /// `self` settles here instead.
    pub settle: bool,
    /// `self`'s choice with the option, if it continues and moves here.
    pub choice: Option<usize>,
}

/// The full `settle` answer.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Settlement {
    /// The evaluated node.
    pub node: NodeIx,
    /// `self`'s certainty equivalent there.
    pub self_value: f64,
    /// The opponent's certainty equivalent there.
    pub opponent_value: f64,
    /// Walk-away prices.
    pub zopa: Zopa,
    /// Predicted prices (`None` = no deal zone).
    pub prices: Option<Prices>,
    /// Bargaining round length (days until the next timed step).
    pub round_days: f64,
    /// Timing along the likely line.
    pub timing: Timing,
    /// Rule 68 evaluation, if an offer was given.
    pub rule68: Option<Rule68Outcome>,
    /// The opponent has its own objective (else zero-sum).
    pub general_sum: bool,
    /// Every fixed point converged.
    pub converged: bool,
    /// Unconverged nodes of the underlying equilibrium solve.
    pub unconverged: Vec<NodeIx>,
    /// Monte Carlo runs that hit the step cap.
    pub truncated_runs: usize,
}

fn check_risk(who: &str, o: &Objective) -> Result<()> {
    match *o {
        Objective::Cvar { alpha, .. } if !(alpha > 0.0 && alpha <= 1.0) => Err(Error::Invalid(
            format!("{who} cvar `alpha` must be in (0, 1], got {alpha}"),
        )),
        _ => Ok(()),
    }
}

fn check(v: &View, o: &SettleOptions) -> Result<()> {
    check_risk("scenario.objective", &v.sc.objective)?;
    check_risk("opponent_risk", &o.opponent_risk)?;
    if !(0.0..=1.0).contains(&o.bargaining_power) {
        return Err(Error::Invalid(format!(
            "bargaining_power must be in [0, 1], got {}",
            o.bargaining_power
        )));
    }
    for d in [o.delta.plaintiff, o.delta.defendant].into_iter().flatten() {
        if !(d > 0.0 && d <= 1.0) {
            return Err(Error::Invalid(format!("delta must be in (0, 1], got {d}")));
        }
    }
    for r in [o.discount_annual.plaintiff, o.discount_annual.defendant]
        .into_iter()
        .flatten()
    {
        if r <= -1.0 {
            return Err(Error::Invalid(format!(
                "discount_annual must be > -1, got {r}"
            )));
        }
    }
    if o.sampling.runs == 0 {
        return Err(Error::Invalid("runs must be at least 1".into()));
    }
    o.rule68.as_ref().map_or(Ok(()), Rule68::check)
}

/// `self` and the opponent as payoff/cost/risk triples.
fn parties(v: &View, cost: &[f64], o: &SettleOptions) -> Result<(Party, Party)> {
    let me = Party {
        terminal: v.utility.clone(),
        cost: cost.to_vec(),
        risk: Risk::of(&v.sc.objective),
    };
    let risk = Risk::of(&o.opponent_risk);
    let opp = match v.sc.opponent_objective.as_deref() {
        Some(src) => {
            let ex = expr::parse(src)?;
            let terminal = (0..v.g.nodes.len())
                .map(|n| {
                    if !v.g.nodes[n].is_terminal() {
                        return Ok(0.0);
                    }
                    ex.eval(&TerminalEnv {
                        g: v.g,
                        n,
                        payoff: v.payoff[n],
                        params: &v.params,
                        path: PathVars::default(),
                    })
                })
                .collect::<Result<_>>()?;
            Party {
                terminal,
                cost: v.metric("opponent_dollars")?,
                risk,
            }
        }
        None => Party {
            terminal: me.terminal.iter().map(|x| -x).collect(),
            cost: me.cost.iter().map(|x| -x).collect(),
            risk,
        },
    };
    Ok((me, opp))
}

/// Solved state shared by every question `settle` answers.
struct Ctx<'a, 'g> {
    v: &'a View<'g>,
    o: &'a SettleOptions,
    choice: std::collections::BTreeMap<NodeIx, usize>,
    me: Party,
    opp: Party,
    ce_me: Vec<f64>,
    ce_opp: Vec<f64>,
    truncated: usize,
}

impl Ctx<'_, '_> {
    /// Range from `self`'s and the opponent's certainty equivalents.
    fn zopa(&self, me: f64, opp: f64) -> Zopa {
        match self.o.self_side {
            Side::Plaintiff => Zopa {
                plaintiff: me,
                defendant: -opp,
            },
            Side::Defendant => Zopa {
                plaintiff: opp,
                defendant: -me,
            },
        }
    }

    /// `(plaintiff, defendant)` CARA coefficients for sure money.
    fn caras(&self) -> (f64, f64) {
        let (a_me, a_opp) = (self.me.risk.cara(), self.opp.risk.cara());
        match self.o.self_side {
            Side::Plaintiff => (a_me, a_opp),
            Side::Defendant => (a_opp, a_me),
        }
    }

    fn nash(&self, z: &Zopa, beta: f64) -> f64 {
        let (a_p, a_d) = self.caras();
        nash_price(z, beta, a_p, a_d)
    }

    /// Certainty equivalents at `n`, with `CVaR` sides estimated by sampling.
    fn ce_at(&mut self, n: NodeIx) -> (f64, f64) {
        let (mut me, mut opp) = (self.ce_me[n], self.ce_opp[n]);
        let cvar_me = matches!(self.me.risk, Risk::Cvar(_));
        let cvar_opp = matches!(self.opp.risk, Risk::Cvar(_));
        if cvar_me || cvar_opp {
            let (xs, t) = cvar_at(
                self.v,
                &self.choice,
                &[&self.me, &self.opp],
                n,
                self.o.sampling,
            );
            self.truncated += t;
            if cvar_me {
                me = xs[0];
            }
            if cvar_opp {
                opp = xs[1];
            }
        }
        (me, opp)
    }

    fn patience(&self, days: f64) -> Patience {
        let rate = |r: Option<f64>| r.or(self.v.sc.discount_annual);
        let (rate_p, rate_d) = (
            rate(self.o.discount_annual.plaintiff),
            rate(self.o.discount_annual.defendant),
        );
        let delta = |d: Option<f64>, r: Option<f64>| {
            d.unwrap_or_else(|| r.map_or(1.0, |r| round_delta(r, days)))
        };
        Patience {
            delta_p: delta(self.o.delta.plaintiff, rate_p),
            delta_d: delta(self.o.delta.defendant, rate_d),
            rate_p,
            rate_d,
        }
    }

    fn timing(&mut self, start: NodeIx) -> (Timing, bool) {
        let v = self.v;
        let beta = self.o.bargaining_power;
        let (sign_me, sign_opp) = match self.o.self_side {
            Side::Plaintiff => (1.0, -1.0),
            Side::Defendant => (-1.0, 1.0),
        };
        let prices: Vec<Option<f64>> = (0..v.g.nodes.len())
            .map(|n| {
                let z = self.zopa(self.ce_me[n], self.ce_opp[n]);
                z.deal().then(|| self.nash(&z, beta))
            })
            .collect();
        let settle = |sign: f64| -> Vec<f64> {
            prices
                .iter()
                .map(|p| p.map_or(f64::NEG_INFINITY, |p| sign * p))
                .collect()
        };
        let me = stopping::solve(v, &self.choice, &self.me, Control::Me, &settle(sign_me));
        let opp = stopping::solve(
            v,
            &self.choice,
            &self.opp,
            Control::Opponent,
            &settle(sign_opp),
        );
        let mut line = vec![];
        let mut changes = vec![];
        for (n, step_p) in likely_line(v, &self.choice, start, self.o.max_steps) {
            let (a, b) = self.ce_at(n);
            let zopa = self.zopa(a, b);
            line.push(LinePoint {
                node: n,
                step_p,
                zopa,
                price: zopa.deal().then(|| self.nash(&zopa, beta)),
                self_continue: me.cont[n],
                self_settles: me.stop[n],
                opponent_settles: opp.stop[n],
            });
            let mine = |c: Option<&usize>| c.copied().filter(|_| v.plan[n].control == Control::Me);
            let (was, now) = (mine(self.choice.get(&n)), mine(me.choice.get(&n)));
            if me.stop[n] || was != now {
                changes.push(Change {
                    node: n,
                    litigation: was,
                    settle: me.stop[n],
                    choice: now,
                });
            }
        }
        let peak = line
            .iter()
            .enumerate()
            .filter(|(_, p)| p.zopa.deal())
            .max_by(|a, b| {
                a.1.zopa
                    .surplus()
                    .total_cmp(&b.1.zopa.surplus())
                    .then(b.0.cmp(&a.0))
            })
            .map(|(i, _)| i);
        let timing = Timing {
            first_settle: line.iter().position(|p| p.self_settles),
            peak,
            line,
            value_litigate: self.ce_me[start],
            value_with_settlement: me.value[start],
            changes,
        };
        (timing, me.converged && opp.converged)
    }
}

/// Predicts the settlement range, price and timing at `node`.
///
/// # Errors
/// Invalid options, an `opponent_objective`/Rule 68 expression that fails to
/// evaluate, or the underlying equilibrium solve's errors.
pub fn settle(v: &View, node: NodeIx, o: &SettleOptions) -> Result<Settlement> {
    check(v, o)?;
    let eq = equilibrium::resolve(v, &SolveOptions::default())?;
    let (me, opp) = parties(v, &eq.solution.cost, o)?;
    let choice = eq.solution.choice.clone();
    let (ce_me, c1) = evaluate(v, &choice, &me);
    let (ce_opp, c2) = evaluate(v, &choice, &opp);
    let mut ctx = Ctx {
        v,
        o,
        choice,
        me,
        opp,
        ce_me,
        ce_opp,
        truncated: 0,
    };
    let (self_value, opponent_value) = ctx.ce_at(node);
    let zopa = ctx.zopa(self_value, opponent_value);
    let days = round_days(v, &ctx.choice)[node];
    let prices = zopa.deal().then(|| Prices {
        midpoint: zopa.midpoint(),
        nash_symmetric: ctx.nash(&zopa, 0.5),
        nash_weighted: ctx.nash(&zopa, o.bargaining_power),
        rubinstein: rubinstein(&zopa, &ctx.patience(days)),
    });
    let (timing, c3) = ctx.timing(node);
    let rule68 = o
        .rule68
        .as_ref()
        .map(|r| rule68::evaluate_offer(&mut ctx, node, r))
        .transpose()?;
    Ok(Settlement {
        node,
        self_value,
        opponent_value,
        zopa,
        prices,
        round_days: days,
        timing,
        rule68,
        general_sum: eq.general_sum,
        converged: c1 && c2 && c3 && eq.solution.converged,
        unconverged: eq.solution.unconverged,
        truncated_runs: ctx.truncated,
    })
}
