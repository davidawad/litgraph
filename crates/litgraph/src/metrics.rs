//! Built-in metrics, terminal utilities, parameters, and the variable
//! environments custom functions evaluate against.
//!
//! A *metric* is an edge expression (cost/weight function). A *utility* is a
//! terminal expression (what ending at a node is worth). Built-ins are plain
//! expressions in these tables; a scenario may add or shadow any of them by
//! name, or pass an expression inline wherever a metric name is accepted.

use crate::error::{Error, Result};
use crate::expr::{Arg, Env};
use crate::model::{Graph, Role};
use std::collections::BTreeMap;

pub struct Builtin {
    pub name: &'static str,
    pub expr: &'static str,
    pub doc: &'static str,
}

pub const METRICS: &[Builtin] = &[
    Builtin { name: "dollars", expr: "hours * rate + fees", doc: "attorney labor at `rate` plus cash fees (the v1 dollarCost)" },
    Builtin { name: "hours", expr: "hours", doc: "attorney hours" },
    Builtin { name: "fees", expr: "fees", doc: "cash outlay: filing/agency fees, bonds" },
    Builtin { name: "days", expr: "days", doc: "deadline window length (v1 `days`). A window, not elapsed time" },
    Builtin { name: "elapsed", expr: "elapsed", doc: "expected calendar days the transition takes (duration.mode, else deadline length, else 0)" },
    Builtin { name: "steps", expr: "1", doc: "number of transitions" },
    Builtin { name: "surprise", expr: "-ln(p)", doc: "-ln(probability); path sum = -ln(path probability), so the shortest path is the most likely line" },
    Builtin { name: "self_dollars", expr: "is_self * (hours * rate + fees)", doc: "only our own spend" },
    Builtin { name: "opponent_dollars", expr: "is_opponent * (hours * opp_rate + fees)", doc: "spend imposed on the opponent (burden you can inflict)" },
    Builtin { name: "hard_deadlines", expr: "has_deadline * (1 - extendable)", doc: "count of non-extendable deadlines (waiver exposure)" },
    Builtin { name: "time_value", expr: "hours * rate + fees + elapsed * carry_per_day", doc: "dollars plus a per-day carrying cost of delay (`carry_per_day`, e.g. ongoing harm or cost of capital)" },
    Builtin { name: "traps", expr: "tag('waiver-trap') + (valence_bad)", doc: "edges that are traps (tagged waiver-trap or valence bad)" },
];

pub const UTILITIES: &[Builtin] = &[
    Builtin {
        name: "ev",
        expr: "payoff * stakes",
        doc: "risk-neutral: payoff scaled by `stakes`",
    },
    Builtin {
        name: "loss_averse",
        expr: "payoff < 0 ? loss_aversion * payoff * stakes : payoff * stakes",
        doc: "prospect-theory style loss aversion (`loss_aversion`, default 2.25)",
    },
    Builtin {
        name: "win_only",
        expr: "tag('win') * payoff",
        doc: "count only terminals tagged win",
    },
];

pub const PARAMS: &[(&str, f64, &str)] = &[
    ("rate", 500.0, "our attorney billing rate, USD/hour"),
    ("opp_rate", 500.0, "opponent billing rate, USD/hour"),
    (
        "stakes",
        1.0,
        "multiplier on terminal payoffs (existential matters > 1)",
    ),
    (
        "loss_aversion",
        2.25,
        "loss multiplier for the loss_averse utility",
    ),
    ("carry_per_day", 0.0, "per-day cost of delay for time_value"),
];

pub const EDGE_VARS: &[(&str, &str)] = &[
    ("hours", "attorney hours on the edge"),
    ("fees / cost", "cash fees on the edge"),
    ("days", "deadline length (0 if none)"),
    (
        "elapsed, elapsed_min, elapsed_max",
        "duration in days (falls back to deadline length)",
    ),
    ("has_deadline, extendable", "0/1"),
    (
        "p",
        "effective probability under the scenario (1 on non-chance edges)",
    ),
    ("p_authored", "authored probability or NaN"),
    (
        "is_self, is_opponent, is_nature",
        "0/1 role of the edge under the scenario's perspective",
    ),
    (
        "is_link, is_synthetic",
        "0/1 cross-pack link / engine-created edge",
    ),
    ("to_terminal, to_payoff", "target is terminal / its payoff"),
    ("valence_good, valence_bad, valence_caution", "0/1"),
    (
        "edge.<attr>",
        "edge attrs (NaN if missing; use attr('x', default))",
    ),
    (
        "from.<attr>, to.<attr>",
        "node attrs of source/target (NaN if missing)",
    ),
    (
        "<param>",
        "any scenario parameter (rate, stakes, ... or your own)",
    ),
];

pub const EDGE_FUNCS: &[(&str, &str)] = &[
    ("tag(\"x\")", "edge has tag x"),
    ("to_tag(\"x\")", "target node has outcome or node tag x"),
    ("from_tag(\"x\")", "source node has outcome or node tag x"),
    ("attr(\"x\", d)", "edge attr x or d"),
    ("actor(\"examiner\")", "edge's raw actor equals"),
    ("pack(\"frcp\")", "edge belongs to pack"),
    (
        "label_has(\"sanction\")",
        "case-insensitive substring of label",
    ),
    (
        "authority_has(\"Rule 37\")",
        "case-insensitive substring of authority",
    ),
];

pub const TERMINAL_VARS: &[(&str, &str)] = &[
    (
        "payoff",
        "terminal payoff (USD, protagonist perspective) after overrides",
    ),
    (
        "payoff_authored",
        "1 if the payoff was authored, 0 if heuristic/default",
    ),
    ("node.<attr>", "node attrs"),
    ("<param>", "any scenario parameter"),
    ("tag(\"win\")", "outcome tag test"),
    ("pack(\"x\"), label_has(\"x\")", "as for edges"),
];

pub fn default_params() -> BTreeMap<String, f64> {
    PARAMS.iter().map(|(k, v, _)| (k.to_string(), *v)).collect()
}

fn b(x: bool) -> f64 {
    if x {
        1.0
    } else {
        0.0
    }
}

fn contains_ci(hay: Option<&str>, needle: &str) -> bool {
    hay.is_some_and(|h| h.to_lowercase().contains(&needle.to_lowercase()))
}

/// Edge evaluation environment.
pub struct EdgeEnv<'a> {
    pub g: &'a Graph,
    pub e: usize,
    pub role: Role,
    pub p: f64,
    pub params: &'a BTreeMap<String, f64>,
    pub to_payoff: f64,
}

impl Env for EdgeEnv<'_> {
    fn var(&self, name: &str) -> Option<f64> {
        let e = &self.g.edges[self.e];
        let dl = e.deadline.as_ref().map(|d| d.length);
        let elapsed = e.duration.as_ref().map(|d| d.mode).or(dl).unwrap_or(0.0);
        Some(match name {
            "hours" => e.hours,
            "fees" | "cost" => e.cost,
            "days" => dl.unwrap_or(0.0),
            "elapsed" => elapsed,
            "elapsed_min" => e.duration.as_ref().and_then(|d| d.min).unwrap_or(elapsed),
            "elapsed_max" => e.duration.as_ref().and_then(|d| d.max).unwrap_or(elapsed),
            "has_deadline" => b(dl.is_some()),
            "extendable" => b(e
                .deadline
                .as_ref()
                .and_then(|d| d.extendable)
                .unwrap_or(false)),
            "p" => self.p,
            "p_authored" => e.probability.unwrap_or(f64::NAN),
            "is_self" => b(self.role == Role::Me),
            "is_opponent" => b(self.role == Role::Opponent),
            "is_nature" => b(self.role == Role::Nature),
            "is_link" => b(e.link),
            "is_synthetic" => b(e.synthetic),
            "to_terminal" => b(self.g.nodes[e.to].is_terminal()),
            "to_payoff" => self.to_payoff,
            "valence_good" => b(e.valence.as_deref() == Some("good")),
            "valence_bad" => b(e.valence.as_deref() == Some("bad")),
            "valence_caution" => b(e.valence.as_deref() == Some("caution")),
            _ => {
                if let Some(a) = name.strip_prefix("edge.") {
                    return Some(e.attrs.get(a).copied().unwrap_or(f64::NAN));
                }
                if let Some(a) = name.strip_prefix("from.") {
                    return Some(
                        self.g.nodes[e.from]
                            .attrs
                            .get(a)
                            .copied()
                            .unwrap_or(f64::NAN),
                    );
                }
                if let Some(a) = name.strip_prefix("to.") {
                    return Some(self.g.nodes[e.to].attrs.get(a).copied().unwrap_or(f64::NAN));
                }
                return self.params.get(name).copied();
            }
        })
    }

    fn func(&self, name: &str, args: &[Arg]) -> Option<Result<f64>> {
        let e = &self.g.edges[self.e];
        let s = || {
            args.first()
                .ok_or_else(|| Error::Expr(format!("{name}() needs an argument")))
                .and_then(|a| a.str().map(str::to_string))
        };
        Some(match name {
            "tag" => s().map(|t| b(e.tags.contains(&t))),
            "to_tag" => s().map(|t| b(self.g.nodes[e.to].has_tag(&t))),
            "from_tag" => s().map(|t| b(self.g.nodes[e.from].has_tag(&t))),
            "actor" => s().map(|t| b(e.actor == t)),
            "pack" => s().map(|t| b(e.pack == t || self.g.nodes[e.from].pack == t)),
            "label_has" => s().map(|t| b(contains_ci(Some(&e.label), &t))),
            "authority_has" => s().map(|t| b(contains_ci(e.authority.as_deref(), &t))),
            "attr" => s().and_then(|k| {
                let d = args.get(1).map(|a| a.num()).transpose()?.unwrap_or(0.0);
                Ok(e.attrs.get(&k).copied().unwrap_or(d))
            }),
            _ => return None,
        })
    }
}

/// Terminal evaluation environment.
pub struct TerminalEnv<'a> {
    pub g: &'a Graph,
    pub n: usize,
    pub payoff: f64,
    pub params: &'a BTreeMap<String, f64>,
}

impl Env for TerminalEnv<'_> {
    fn var(&self, name: &str) -> Option<f64> {
        let n = &self.g.nodes[self.n];
        match name {
            "payoff" => Some(self.payoff),
            "payoff_authored" => Some(b(n.payoff_source == crate::model::PayoffSource::Authored)),
            _ => {
                if let Some(a) = name.strip_prefix("node.") {
                    return Some(n.attrs.get(a).copied().unwrap_or(f64::NAN));
                }
                self.params.get(name).copied()
            }
        }
    }
    fn func(&self, name: &str, args: &[Arg]) -> Option<Result<f64>> {
        let n = &self.g.nodes[self.n];
        let s = || {
            args.first()
                .ok_or_else(|| Error::Expr(format!("{name}() needs an argument")))
                .and_then(|a| a.str().map(str::to_string))
        };
        Some(match name {
            "tag" | "to_tag" => s().map(|t| b(n.has_tag(&t))),
            "pack" => s().map(|t| b(n.pack == t)),
            "label_has" => s().map(|t| b(contains_ci(Some(&n.label), &t))),
            _ => return None,
        })
    }
}
