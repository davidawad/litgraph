// SPDX-License-Identifier: GPL-3.0-or-later
//! The `settle` op: renders [`crate::algo::settle`] with ids, labels,
//! rounded numbers, and a warning for every modeling fallback taken.

use serde_json::{json, Value};

use super::render::{choice_id, choice_label, node_ref, r};
use super::{SettleArgs, Warn};
use crate::algo::settle::bargain::Zopa;
use crate::algo::settle::value::Sampling;
use crate::algo::settle::{settle, Rule68Outcome, SettleOptions, Settlement, Side, Timing};
use crate::error::Result;
use crate::model::NodeIx;
use crate::scenario::{Objective, View};

type Out = Result<(Value, Vec<Warn>)>;

fn zopa_json(z: &Zopa) -> Value {
    let mut j = json!({
        "plaintiff_reservation": r(z.plaintiff),
        "defendant_reservation": r(z.defendant),
        "deal": z.deal(),
    });
    if z.deal() {
        j["zopa"] =
            json!({ "low": r(z.plaintiff), "high": r(z.defendant), "surplus": r(z.surplus()) });
    } else {
        j["no_deal_gap"] = r(-z.surplus());
    }
    j
}

fn warn(code: &str, at: Option<String>, message: String) -> Warn {
    Warn {
        code: code.into(),
        at,
        message,
    }
}

fn warnings(v: &View, s: &Settlement, o: &SettleOptions) -> Vec<Warn> {
    let mut w = vec![];
    if !s.general_sum {
        w.push(warn("settle-zero-sum", None, "scenario.opponent_objective is unset, so the opponent's value is the negation of ours and every range has zero surplus (no deal). Set it to the opponent's own terminal payoff, e.g. \"-payoff\" for a defendant who pays the judgment".into()));
    }
    let cvar_side = [("self", &v.sc.objective), ("opponent", &o.opponent_risk)]
        .into_iter()
        .filter(|(_, x)| matches!(x, Objective::Cvar { .. }))
        .map(|(who, _)| who)
        .collect::<Vec<_>>();
    if !cvar_side.is_empty() {
        w.push(warn("settle-cvar-sampled", None, format!(
            "{} walk-away value is CVaR, estimated by Monte Carlo ({} runs, seed {}) at the evaluated node and along the timing line; the stopping policy and prices off the line use the expectation for that side",
            cvar_side.join(" and "), o.sampling.runs, o.sampling.seed
        )));
    }
    if matches!(v.sc.objective, Objective::Cvar { .. }) {
        w.push(warn("settle-cvar-policy", None, "scenario.objective is cvar: the litigation policy comes from the expected-value equilibrium (CVaR does not compose with a general-sum opponent); CVaR only prices self's walk-away point".into()));
    }
    if s.truncated_runs > 0 {
        w.push(warn("settle-truncated", None, format!("{} Monte Carlo runs hit the step cap without ending; their outcome counts only the costs accrued", s.truncated_runs)));
    }
    if !s.converged {
        w.push(warn("not-converged", None, "a cyclic component hit the iteration cap in the equilibrium, walk-away, or stopping pass; values there are unreliable".into()));
    }
    for &n in &s.unconverged {
        w.push(warn(
            "not-converged",
            Some(v.g.nodes[n].id.clone()),
            "value iteration hit the cap in a cycle".into(),
        ));
    }
    w
}

fn choice_json(v: &View, e: Option<usize>) -> Value {
    e.map_or(
        Value::Null,
        |e| json!({ "edge": choice_id(v, e), "label": choice_label(v, e) }),
    )
}

fn node_id(v: &View, n: NodeIx) -> Value {
    json!(v.g.nodes[n].id)
}

fn options(a: &SettleArgs) -> SettleOptions {
    SettleOptions {
        self_side: a.self_side,
        opponent_risk: a.opponent_risk.clone(),
        bargaining_power: a.bargaining_power,
        discount_annual: a.discount_annual,
        delta: a.delta,
        rule68: a.rule68.clone(),
        sampling: Sampling {
            runs: a.runs,
            seed: a.seed,
        },
        max_steps: a.max_steps,
    }
}

fn timing_json(v: &View, t: &Timing) -> Value {
    let line: Vec<Value> = t
        .line
        .iter()
        .map(|p| {
            let mut j = zopa_json(&p.zopa);
            j["node"] = node_ref(v, p.node);
            j["step_p"] = r(p.step_p);
            j["surplus"] = r(p.zopa.surplus());
            j["price"] = p.price.map_or(Value::Null, r);
            j["self_continue"] = r(p.self_continue);
            j["self_settles"] = json!(p.self_settles);
            j["opponent_settles"] = json!(p.opponent_settles);
            j
        })
        .collect();
    let at = |i: Option<usize>| i.map_or(Value::Null, |i| node_id(v, t.line[i].node));
    json!({
        "peak": at(t.peak),
        "peak_surplus": t.peak.map_or(Value::Null, |i| r(t.line[i].zopa.surplus())),
        "first_settle": at(t.first_settle),
        "value_litigate": r(t.value_litigate),
        "value_with_settlement": r(t.value_with_settlement),
        "settlement_option_value": r(t.value_with_settlement - t.value_litigate),
        "policy_changes": t.changes.iter().map(|c| json!({
            "node": node_id(v, c.node),
            "litigation": choice_json(v, c.litigation),
            "with_settlement": if c.settle { json!("settle") } else { choice_json(v, c.choice) },
        })).collect::<Vec<_>>(),
        "line": line,
    })
}

fn rule68_json(v: &View, x: &Rule68Outcome, self_side: Side) -> Value {
    let mut j = zopa_json(&x.zopa);
    j["offer"] = r(x.offer);
    j["costs"] = r(x.costs);
    j["p_triggered"] = r(x.p_triggered);
    j["triggered"] = json!(x
        .triggered
        .iter()
        .map(|&t| node_id(v, t))
        .collect::<Vec<_>>());
    j["plaintiff_reject_value"] = r(x.plaintiff_reject_value);
    j["defendant_reject_value"] = r(x.defendant_reject_value);
    j["plaintiff_accepts"] = json!(x.accept);
    j["self_is_offeror"] = json!(self_side == Side::Defendant);
    j
}

pub(super) fn settle_op(v: &View, a: &SettleArgs) -> Out {
    let n = match &a.node {
        Some(x) => v.g.node(x)?,
        None => v.start,
    };
    let o = &options(a);
    let s = settle(v, n, o)?;
    let mut result = zopa_json(&s.zopa);
    result["node"] = node_ref(v, s.node);
    result["self_side"] = json!(o.self_side);
    result["general_sum"] = json!(s.general_sum);
    result["values"] = json!({
        "self": r(s.self_value),
        "opponent": r(s.opponent_value),
        "self_risk": v.sc.objective,
        "opponent_risk": o.opponent_risk,
    });
    result["prices"] = s.prices.map_or(Value::Null, |p| {
        json!({
            "midpoint": r(p.midpoint),
            "nash_symmetric": r(p.nash_symmetric),
            "nash_weighted": { "bargaining_power": o.bargaining_power, "price": r(p.nash_weighted) },
            "rubinstein": {
                "round_days": r(s.round_days),
                "delta_plaintiff": r(p.rubinstein.delta_plaintiff),
                "delta_defendant": r(p.rubinstein.delta_defendant),
                "plaintiff_first": r(p.rubinstein.plaintiff_first),
                "defendant_first": r(p.rubinstein.defendant_first),
                "limit": r(p.rubinstein.limit),
                "limit_plaintiff_share": r(p.rubinstein.limit_share),
            },
        })
    });
    result["timing"] = timing_json(v, &s.timing);
    result["rule68"] = s
        .rule68
        .as_ref()
        .map_or(Value::Null, |x| rule68_json(v, x, o.self_side));
    Ok((result, warnings(v, &s, o)))
}
