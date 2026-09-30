// SPDX-License-Identifier: GPL-3.0-or-later
//! Offer of judgment: FRCP 68 (and its identical Court of Federal Claims
//! counterpart, RCFC 68).
//!
//! Rule 68(a) lets "a party defending against a claim" serve an offer to
//! allow judgment on specified terms; 68(d): "If the judgment that the
//! offeree finally obtains is not more favorable than the unaccepted offer,
//! the offeree must pay the costs incurred after the offer was made." It
//! applies only where the plaintiff obtains a judgment — not when judgment
//! is for the defendant (*Delta Air Lines, Inc. v. August*, 450 U.S. 346
//! (1981)) — and "costs" means whatever the substantive statute makes
//! awardable as costs, attorney's fees included only where that statute
//! says so (*Marek v. Chesny*, 473 U.S. 1 (1985)). Sources and URLs are in
//! `docs/SETTLEMENT.md`.
//!
//! Model: the defendant serves the offer at the evaluated node. At every
//! terminal where 68(d) bites (default: tagged `judgment`, the plaintiff's
//! value positive and at most the offer), the plaintiff pays the defendant
//! `costs`. Both sides' walk-away values are re-evaluated under the same
//! solved policy; the plaintiff accepts if the offer is at least its value
//! of rejecting.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use super::bargain::Zopa;
use super::value::{evaluate, Party, Risk};
use super::{Ctx, Side};
use crate::algo::mdp::absorb_prob;
use crate::error::{Error, Result};
use crate::expr;
use crate::metrics::{PathVars, TerminalEnv};
use crate::model::NodeIx;

/// A Rule 68 offer of judgment, served by the defendant at the evaluated node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rule68 {
    /// The offered judgment amount (USD, what the defendant would pay).
    pub offer: f64,
    /// The defendant's post-offer costs the plaintiff would pay under
    /// 68(d) (USD; your estimate — taxable costs, plus attorney's fees only
    /// where the substantive statute defines them as costs).
    #[serde(default)]
    pub costs: f64,
    /// Terminal expression overriding which endings trigger 68(d) (nonzero
    /// = triggers; the parameter `offer` holds the offer). Default: tagged
    /// `judgment`, the plaintiff's value positive and at most `offer`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eligible: Option<String>,
}

impl Rule68 {
    pub(super) fn check(&self) -> Result<()> {
        if self.costs < 0.0 {
            return Err(Error::Invalid(format!(
                "rule68.costs must be >= 0, got {}",
                self.costs
            )));
        }
        Ok(())
    }
}

/// What an offer of judgment does to the range.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Rule68Outcome {
    /// The offer.
    pub offer: f64,
    /// Costs shifted at a triggering judgment.
    pub costs: f64,
    /// Probability the case ends in a judgment that triggers 68(d).
    pub p_triggered: f64,
    /// Terminals where 68(d) bites.
    pub triggered: Vec<NodeIx>,
    /// Plaintiff's value of rejecting the offer (with the cost exposure).
    pub plaintiff_reject_value: f64,
    /// Defendant's value if the offer is rejected (costs recovered).
    pub defendant_reject_value: f64,
    /// Walk-away prices with the offer outstanding.
    pub zopa: Zopa,
    /// The plaintiff should accept (the offer is at least its value of rejecting).
    pub accept: bool,
}

/// Evaluates `r` served at `node`.
pub(super) fn evaluate_offer(ctx: &mut Ctx, node: NodeIx, r: &Rule68) -> Result<Rule68Outcome> {
    let v = ctx.v;
    let (plaintiff, defendant) = match ctx.o.self_side {
        Side::Plaintiff => (&ctx.me, &ctx.opp),
        Side::Defendant => (&ctx.opp, &ctx.me),
    };
    let custom = r.eligible.as_deref().map(expr::parse).transpose()?;
    let mut params: BTreeMap<String, f64> = v.params.clone();
    params.insert("offer".into(), r.offer);
    let triggered: Vec<NodeIx> =
        v.g.terminals()
            .map(|t| {
                let hit = if let Some(ex) = &custom {
                    ex.eval(&TerminalEnv {
                        g: v.g,
                        n: t,
                        payoff: v.payoff[t],
                        params: &params,
                        path: PathVars::default(),
                    })? != 0.0
                } else {
                    let x = plaintiff.terminal[t];
                    v.g.nodes[t].has_tag("judgment") && x > 0.0 && x <= r.offer
                };
                Ok((t, hit))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter_map(|(t, hit)| hit.then_some(t))
            .collect();
    let shift = |p: &Party, sign: f64| {
        let mut q = p.clone();
        for &t in &triggered {
            q.terminal[t] += sign * r.costs;
        }
        q
    };
    let (p68, d68) = (shift(plaintiff, -1.0), shift(defendant, 1.0));
    let mut value = |p: &Party| -> f64 {
        if matches!(p.risk, Risk::Cvar(_)) {
            let (xs, t) = super::value::cvar_at(v, &ctx.choice, &[p], node, ctx.o.sampling);
            ctx.truncated += t;
            xs[0]
        } else {
            evaluate(v, &ctx.choice, p).0[node]
        }
    };
    let (pv, dv) = (value(&p68), value(&d68));
    let mut target = vec![0.0; v.g.nodes.len()];
    for &t in &triggered {
        target[t] = 1.0;
    }
    Ok(Rule68Outcome {
        offer: r.offer,
        costs: r.costs,
        p_triggered: absorb_prob(v, &ctx.choice, &target)[node],
        triggered,
        plaintiff_reject_value: pv,
        defendant_reject_value: dv,
        zopa: Zopa {
            plaintiff: pv,
            defendant: -dv,
        },
        accept: r.offer >= pv,
    })
}
