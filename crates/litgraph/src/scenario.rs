//! Scenario = everything that parametrizes an analysis without editing packs:
//! parameters, custom metrics, perspective, masks (waivers / counterfactuals),
//! probability and payoff overrides, and the modeling choices that v1 buried
//! in code (mixed-node semantics, opponent model, probability fill).
//!
//! `View::new(graph, scenario)` resolves a scenario into dense per-edge /
//! per-node arrays that every algorithm consumes, and records every fallback
//! it had to take as a structured `Warning`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::error::{Error, Result};
use crate::expr::{self, Expr};
use crate::metrics::{self, EdgeEnv, TerminalEnv};
use crate::model::{Graph, NodeIx, PayoffSource, Role};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum MixedMode {
    /// Chance/interrupt edges fire with their authored probabilities; with the
    /// residual mass the chooser picks. Falls back to `act-or-wait` when the
    /// interrupts carry no probabilities (or take all the mass).
    #[default]
    NatureFirst,
    /// The chooser picks one of its own edges, or waits and lets the world
    /// edges fire (probabilities filled). "Discovery proceeds unless you move
    /// to compel" reads this way.
    ActOrWait,
    /// Chooser picks among its own edges; interrupts are ignored (v1 absorbing-chain semantics).
    SelfOnly,
    /// Chooser may take ANY out-edge, including the world's (v1 value-iteration semantics).
    Optimistic,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum OpponentMode {
    /// Authored probabilities if complete, else adversarial.
    #[default]
    Auto,
    /// Opponent minimizes our value (zero-sum).
    Adversarial,
    /// Opponent's choices are draws (probability fill applies).
    Chance,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ProbFill {
    /// Authored probabilities kept; unauthored siblings split the residual equally.
    #[default]
    Residual,
    /// Any missing probability → uniform over all siblings (v1 behavior).
    Uniform,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
#[derive(Default)]
pub enum Objective {
    /// Maximize expected (utility − cost).
    #[default]
    Expected,
    /// Exponential utility with absolute risk aversion `a` (per dollar).
    /// Solved exactly via certainty equivalents; a > 0 is risk-averse.
    Cara { a: f64 },
    /// Nature is adversarial too (robust / worst case).
    Worst,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FeeShift {
    /// Fraction of our accumulated cost recovered at an eligible terminal.
    pub fraction: f64,
    /// Terminal expression selecting eligible terminals. Default `tag("fee-eligible")`.
    #[serde(default)]
    pub eligible: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "snake_case")]
pub struct Scenario {
    /// Parameter values visible to every expression (defaults: see `describe`).
    pub params: BTreeMap<String, f64>,
    /// Named custom edge metrics (name -> expression). Shadow built-ins.
    pub metrics: BTreeMap<String, String>,
    /// Named custom terminal utilities (name -> expression).
    pub utilities: BTreeMap<String, String>,
    /// Metric used as the cost in solve/chain/simulate (name or expression). Default `dollars`.
    pub cost: Option<String>,
    /// Terminal utility (name or expression). Default `ev`.
    pub utility: Option<String>,
    /// actor -> role override for this analysis (e.g. analyze as the defendant).
    pub perspective: BTreeMap<String, Role>,
    /// Edge expression; edges where it is 0 are removed (counterfactual / waiver).
    pub mask: Option<String>,
    /// Edge refs to delete.
    pub remove_edges: Vec<String>,
    /// Edge ref -> probability. Siblings are rescaled to keep the node summing to 1.
    pub probabilities: BTreeMap<String, f64>,
    /// Edge expression rewriting the probability of every non-self edge, e.g.
    /// `label_has("grant") ? p * 1.5 : p` for a judge who grants more. `p` is
    /// the authored (or overridden) probability, NaN if none; a non-finite
    /// result keeps the original. Chance nodes are renormalized afterwards.
    pub probability_fn: Option<String>,
    /// Node ref -> payoff override (USD).
    pub payoffs: BTreeMap<String, f64>,
    /// Node ref -> edge ref: force our choice at these nodes.
    pub policy: BTreeMap<String, String>,
    pub mixed: MixedMode,
    pub opponent: OpponentMode,
    pub prob_fill: ProbFill,
    pub objective: Objective,
    /// Annual discount rate applied along `elapsed` time (e.g. 0.08).
    pub discount_annual: Option<f64>,
    pub fee_shift: Option<FeeShift>,
    /// Start node (default: the first pack's start).
    pub start: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Control {
    Terminal,
    /// Non-terminal with no active out-edges. Value 0; always a content bug or a mask effect.
    Sink,
    Chance,
    Me,
    Opponent,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Warning {
    pub code: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    pub message: String,
}

/// How a node's out-edges are split between an interrupting draw and a choice.
#[derive(Debug, Clone)]
pub struct NodePlan {
    pub control: Control,
    /// Edges the chooser (Me / Opponent) may pick from.
    pub choices: Vec<usize>,
    /// (edge, probability) that fire before/instead of the choice. For Chance
    /// nodes this is the whole distribution.
    pub draws: Vec<(usize, f64)>,
    /// Probability mass left for the choice (1 − Σ draws) at mixed nodes.
    pub choice_mass: f64,
    /// Chooser minimizes (adversarial opponent / worst-case nature).
    pub minimize: bool,
    /// Act-or-wait: the chooser may instead WAIT, letting these world edges
    /// fire with these probabilities (empty = no wait option).
    pub wait: Vec<(usize, f64)>,
}

pub struct View<'g> {
    pub g: &'g Graph,
    pub sc: Scenario,
    pub params: BTreeMap<String, f64>,
    pub active: Vec<bool>,
    pub role: Vec<Role>,
    /// Effective probability of each edge within its node's draw distribution (None if a choice).
    pub prob: Vec<Option<f64>>,
    pub payoff: Vec<f64>,
    /// Terminal utility per node (0 off-terminal).
    pub utility: Vec<f64>,
    /// Scenario cost metric per edge.
    pub cost: Vec<f64>,
    /// Expected elapsed days per edge.
    pub elapsed: Vec<f64>,
    pub plan: Vec<NodePlan>,
    /// Unweighted steps from each node to the nearest terminal over active edges (u32::MAX if none).
    pub dist_to_terminal: Vec<u32>,
    pub forced: BTreeMap<NodeIx, usize>,
    pub start: NodeIx,
    pub warnings: Vec<Warning>,
}

/// Resolve a metric/utility spec: a registered name or an inline expression.
pub fn resolve_spec<'a>(
    spec: &'a str,
    custom: &'a BTreeMap<String, String>,
    builtins: &'a [metrics::Builtin],
) -> &'a str {
    if let Some(src) = custom.get(spec) {
        return src;
    }
    builtins
        .iter()
        .find(|b| b.name == spec)
        .map(|b| b.expr)
        .unwrap_or(spec)
}

impl<'g> View<'g> {
    pub fn new(g: &'g Graph, sc: &Scenario) -> Result<View<'g>> {
        let mut params = metrics::default_params();
        params.extend(sc.params.clone());
        if !sc.params.contains_key("opp_rate") {
            params.insert("opp_rate".into(), params["rate"]);
        }
        let mut warnings = vec![];
        let ne = g.edges.len();
        let nn = g.nodes.len();

        // Roles.
        let role: Vec<Role> = (0..ne)
            .map(|e| {
                sc.perspective
                    .get(&g.edges[e].actor)
                    .copied()
                    .unwrap_or_else(|| g.base_role(e))
            })
            .collect();

        // Payoffs.
        let mut payoff: Vec<f64> = g.nodes.iter().map(|n| n.payoff).collect();
        for (r, v) in &sc.payoffs {
            payoff[g.node(r)?] = *v;
        }

        // Active edges: mask expression + removals.
        let mut active = vec![true; ne];
        for r in &sc.remove_edges {
            active[g.edge(r)?] = false;
        }
        let mask = sc.mask.as_deref().map(expr::parse).transpose()?;
        if let Some(m) = &mask {
            for e in 0..ne {
                let env = EdgeEnv {
                    g,
                    e,
                    role: role[e],
                    p: g.edges[e].probability.unwrap_or(1.0),
                    params: &params,
                    to_payoff: payoff[g.edges[e].to],
                };
                if m.eval(&env)? == 0.0 {
                    active[e] = false;
                }
            }
        }

        // Probability overrides.
        let mut authored: Vec<Option<f64>> = g.edges.iter().map(|e| e.probability).collect();
        for (r, p) in &sc.probabilities {
            let e = g.edge(r)?;
            if !(0.0..=1.0).contains(p) {
                return Err(Error::Invalid(format!(
                    "probability for {r} must be in [0,1]"
                )));
            }
            let from = g.edges[e].from;
            let others: Vec<usize> = g.out[from]
                .iter()
                .copied()
                .filter(|&o| o != e && authored[o].is_some())
                .collect();
            let rest: f64 = others.iter().map(|&o| authored[o].unwrap()).sum();
            let old = authored[e].unwrap_or(0.0);
            authored[e] = Some(*p);
            // Rescale authored siblings so the node keeps its total mass.
            if rest > 0.0 {
                let target_rest = (rest + old - p).max(0.0);
                for o in others {
                    authored[o] = Some(authored[o].unwrap() * target_rest / rest);
                }
            }
        }

        if let Some(src) = &sc.probability_fn {
            let ex = expr::parse(src)?;
            for e in 0..ne {
                if !active[e] || role[e] == Role::Me {
                    continue;
                }
                let env = EdgeEnv {
                    g,
                    e,
                    role: role[e],
                    p: authored[e].unwrap_or(f64::NAN),
                    params: &params,
                    to_payoff: payoff[g.edges[e].to],
                };
                let x = ex.eval(&env).map_err(|err| {
                    Error::Expr(format!("probability_fn on {}: {err}", g.edges[e].id))
                })?;
                if x.is_finite() {
                    authored[e] = Some(x.clamp(0.0, 1.0));
                }
            }
        }

        // Forced policy.
        let mut forced = BTreeMap::new();
        for (n, e) in &sc.policy {
            let ni = g.node(n)?;
            forced.insert(ni, g.edge_at(ni, e)?);
        }

        // Node plans.
        let mut plan = Vec::with_capacity(nn);
        let mut prob: Vec<Option<f64>> = vec![None; ne];
        let worst = sc.objective == Objective::Worst;
        for n in 0..nn {
            let outs: Vec<usize> = g.out[n].iter().copied().filter(|&e| active[e]).collect();
            let at = || Some(g.nodes[n].id.clone());
            if g.nodes[n].is_terminal() {
                plan.push(NodePlan {
                    control: Control::Terminal,
                    choices: vec![],
                    draws: vec![],
                    choice_mass: 0.0,
                    minimize: false,
                    wait: vec![],
                });
                continue;
            }
            if outs.is_empty() {
                warnings.push(Warning {
                    code: "sink",
                    at: at(),
                    message: "non-terminal with no active out-edges; valued at 0".into(),
                });
                plan.push(NodePlan {
                    control: Control::Sink,
                    choices: vec![],
                    draws: vec![],
                    choice_mass: 0.0,
                    minimize: false,
                    wait: vec![],
                });
                continue;
            }
            let of = |r: Role| {
                outs.iter()
                    .copied()
                    .filter(|&e| role[e] == r)
                    .collect::<Vec<_>>()
            };
            let (mine, opp, nat) = (of(Role::Me), of(Role::Opponent), of(Role::Nature));

            // Who chooses here, and what interrupts them.
            let (chooser, choices, interrupts): (Option<Role>, Vec<usize>, Vec<usize>) =
                if !mine.is_empty() {
                    (
                        Some(Role::Me),
                        mine.clone(),
                        [opp.clone(), nat.clone()].concat(),
                    )
                } else if !opp.is_empty() {
                    let opp_probs_complete = opp.iter().all(|&e| authored[e].is_some());
                    let as_chance = match sc.opponent {
                        OpponentMode::Chance => true,
                        OpponentMode::Adversarial => false,
                        OpponentMode::Auto => opp_probs_complete,
                    };
                    if as_chance {
                        (None, vec![], outs.clone())
                    } else {
                        (Some(Role::Opponent), opp.clone(), nat.clone())
                    }
                } else {
                    (None, vec![], nat.clone())
                };

            match chooser {
                None => {
                    // Pure chance node over `interrupts` (all outs).
                    let dist = fill(&interrupts, &authored, sc.prob_fill);
                    if interrupts.iter().any(|&e| authored[e].is_none()) {
                        warnings.push(Warning {
                            code: "probability-fill",
                            at: at(),
                            message: format!(
                                "{} of {} out-edges lack probabilities; filled ({:?})",
                                interrupts
                                    .iter()
                                    .filter(|&&e| authored[e].is_none())
                                    .count(),
                                interrupts.len(),
                                sc.prob_fill
                            ),
                        });
                    } else {
                        let s: f64 = interrupts.iter().map(|&e| authored[e].unwrap()).sum();
                        if (s - 1.0).abs() > 1e-3 {
                            warnings.push(Warning { code: "probability-renormalized", at: at(), message: format!("authored probabilities sum to {s:.3} after masking; renormalized") });
                        }
                    }
                    for &(e, p) in &dist {
                        prob[e] = Some(p);
                    }
                    if worst {
                        plan.push(NodePlan {
                            control: Control::Chance,
                            choices: interrupts.clone(),
                            draws: vec![],
                            choice_mass: 1.0,
                            minimize: true,
                            wait: vec![],
                        });
                    } else {
                        plan.push(NodePlan {
                            control: Control::Chance,
                            choices: vec![],
                            draws: dist,
                            choice_mass: 0.0,
                            minimize: false,
                            wait: vec![],
                        });
                    }
                }
                Some(who) => {
                    let control = if who == Role::Me {
                        Control::Me
                    } else {
                        Control::Opponent
                    };
                    let minimize = who == Role::Opponent;
                    if interrupts.is_empty() {
                        plan.push(NodePlan {
                            control,
                            choices,
                            draws: vec![],
                            choice_mass: 1.0,
                            minimize,
                            wait: vec![],
                        });
                        continue;
                    }
                    match sc.mixed {
                        MixedMode::Optimistic => {
                            plan.push(NodePlan {
                                control,
                                choices: outs.clone(),
                                draws: vec![],
                                choice_mass: 1.0,
                                minimize,
                                wait: vec![],
                            });
                        }
                        MixedMode::SelfOnly => {
                            plan.push(NodePlan {
                                control,
                                choices,
                                draws: vec![],
                                choice_mass: 1.0,
                                minimize,
                                wait: vec![],
                            });
                        }
                        MixedMode::NatureFirst => {
                            let complete = interrupts.iter().all(|&e| authored[e].is_some());
                            let mass: f64 = interrupts.iter().filter_map(|&e| authored[e]).sum();
                            if complete && mass < 1.0 - 1e-9 {
                                let draws: Vec<(usize, f64)> = interrupts
                                    .iter()
                                    .map(|&e| (e, authored[e].unwrap()))
                                    .collect();
                                for &(e, p) in &draws {
                                    prob[e] = Some(p);
                                }
                                plan.push(NodePlan {
                                    control,
                                    choices,
                                    draws,
                                    choice_mass: 1.0 - mass,
                                    minimize,
                                    wait: vec![],
                                });
                            } else {
                                warnings.push(Warning {
                                    code: "mixed-node",
                                    at: at(),
                                    message: format!(
                                        "{} may act here, and {} world edge(s) {}; modeled as act-or-wait (act, or let the world edges fire). Author probabilities summing < 1 on them to model a true interrupt",
                                        if who == Role::Me { "self" } else { "opponent" },
                                        interrupts.len(),
                                        if complete { "take all the probability mass" } else { "have no probabilities" }
                                    ),
                                });
                                plan.push(act_or_wait(
                                    control,
                                    choices,
                                    &interrupts,
                                    &authored,
                                    sc.prob_fill,
                                    minimize,
                                    &mut prob,
                                ));
                            }
                        }
                        MixedMode::ActOrWait => {
                            plan.push(act_or_wait(
                                control,
                                choices,
                                &interrupts,
                                &authored,
                                sc.prob_fill,
                                minimize,
                                &mut prob,
                            ));
                        }
                    }
                }
            }
        }

        // Utilities.
        let util_spec = sc.utility.clone().unwrap_or_else(|| "ev".into());
        let util_src = resolve_spec(&util_spec, &sc.utilities, metrics::UTILITIES);
        let util_expr = expr::parse(util_src)?;
        let mut utility = vec![0.0; nn];
        let mut heuristic_terms = 0;
        for n in 0..nn {
            if g.nodes[n].is_terminal() {
                let env = TerminalEnv {
                    g,
                    n,
                    payoff: payoff[n],
                    params: &params,
                };
                utility[n] = util_expr.eval(&env).map_err(|e| {
                    Error::Expr(format!("utility `{util_src}` at {}: {e}", g.nodes[n].id))
                })?;
                if g.nodes[n].payoff_source != PayoffSource::Authored
                    && !sc.payoffs.keys().any(|k| g.node(k).ok() == Some(n))
                {
                    heuristic_terms += 1;
                }
            }
        }
        if heuristic_terms > 0 {
            warnings.push(Warning {
                code: "payoff-not-authored",
                at: None,
                message: format!("{heuristic_terms} terminal(s) use a heuristic or zero payoff; override with scenario.payoffs or author `payoff` in the pack"),
            });
        }

        let mut dist_to_terminal = vec![u32::MAX; nn];
        let mut queue: std::collections::VecDeque<NodeIx> =
            (0..nn).filter(|&n| g.nodes[n].is_terminal()).collect();
        for &t in &queue {
            dist_to_terminal[t] = 0;
        }
        while let Some(u) = queue.pop_front() {
            for &e in &g.inc[u] {
                let w = g.edges[e].from;
                if active[e] && dist_to_terminal[w] == u32::MAX {
                    dist_to_terminal[w] = dist_to_terminal[u] + 1;
                    queue.push_back(w);
                }
            }
        }
        let mut view = View {
            g,
            sc: sc.clone(),
            params,
            active,
            role,
            prob,
            payoff,
            utility,
            cost: vec![0.0; ne],
            elapsed: vec![0.0; ne],
            plan,
            dist_to_terminal,
            forced,
            start: 0,
            warnings,
        };
        view.start = match &sc.start {
            Some(s) => g.node(s)?,
            None => g.start,
        };
        let cost_spec = sc.cost.clone().unwrap_or_else(|| "dollars".into());
        view.cost = view.metric(&cost_spec)?;
        view.elapsed = view.metric("elapsed")?;
        if view
            .cost
            .iter()
            .zip(&view.active)
            .any(|(c, a)| *a && *c < 0.0)
        {
            view.warnings.push(Warning {
                code: "negative-cost",
                at: None,
                message: format!(
                    "cost metric `{cost_spec}` is negative on some edges (treated as a reward)"
                ),
            });
        }
        Ok(view)
    }

    /// Evaluate a metric (name or expression) on every edge. Inactive edges get NaN.
    pub fn metric(&self, spec: &str) -> Result<Vec<f64>> {
        let src = resolve_spec(spec, &self.sc.metrics, metrics::METRICS);
        let ex: Expr = expr::parse(src)?;
        (0..self.g.edges.len())
            .map(|e| {
                if !self.active[e] {
                    return Ok(f64::NAN);
                }
                let env = self.edge_env(e);
                ex.eval(&env).map_err(|err| {
                    Error::Expr(format!(
                        "metric `{src}` on edge {}: {err}",
                        self.g.edges[e].id
                    ))
                })
            })
            .collect()
    }

    /// Evaluate a terminal expression on every node (non-terminals get NaN).
    pub fn terminal_metric(&self, spec: &str) -> Result<Vec<f64>> {
        let src = resolve_spec(spec, &self.sc.utilities, metrics::UTILITIES);
        let ex = expr::parse(src)?;
        (0..self.g.nodes.len())
            .map(|n| {
                if !self.g.nodes[n].is_terminal() {
                    return Ok(f64::NAN);
                }
                ex.eval(&TerminalEnv {
                    g: self.g,
                    n,
                    payoff: self.payoff[n],
                    params: &self.params,
                })
            })
            .collect()
    }

    pub fn edge_env(&self, e: usize) -> EdgeEnv<'_> {
        EdgeEnv {
            g: self.g,
            e,
            role: self.role[e],
            p: self.prob[e].unwrap_or(1.0),
            params: &self.params,
            to_payoff: self.payoff[self.g.edges[e].to],
        }
    }

    /// Active out-edges of a node.
    pub fn outs(&self, n: NodeIx) -> impl Iterator<Item = usize> + '_ {
        self.g.out[n]
            .iter()
            .copied()
            .filter(move |&e| self.active[e])
    }

    pub fn set_param(&mut self, name: &str, v: f64) -> Result<()> {
        let mut sc = self.sc.clone();
        sc.params.insert(name.into(), v);
        let fresh = View::new(self.g, &sc)?;
        *self = fresh;
        Ok(())
    }
}

/// Sentinel "edge" recorded in a solution's choice map when the chooser waits.
pub const WAIT: usize = usize::MAX;

/// The chooser picks one of its own edges or WAITs, in which case the world
/// edges fire with their (filled) probabilities.
fn act_or_wait(
    control: Control,
    choices: Vec<usize>,
    interrupts: &[usize],
    authored: &[Option<f64>],
    fill_mode: ProbFill,
    minimize: bool,
    prob: &mut [Option<f64>],
) -> NodePlan {
    let wait = fill(interrupts, authored, fill_mode);
    for &(e, p) in &wait {
        prob[e] = Some(p);
    }
    NodePlan {
        control,
        choices,
        draws: vec![],
        choice_mass: 1.0,
        minimize,
        wait,
    }
}

/// Probability fill for a draw over `edges`.
fn fill(edges: &[usize], authored: &[Option<f64>], mode: ProbFill) -> Vec<(usize, f64)> {
    let n = edges.len() as f64;
    let missing = edges.iter().filter(|&&e| authored[e].is_none()).count();
    let sum: f64 = edges.iter().filter_map(|&e| authored[e]).sum();
    if missing == 0 {
        if sum <= 0.0 {
            return edges.iter().map(|&e| (e, 1.0 / n)).collect();
        }
        return edges
            .iter()
            .map(|&e| (e, authored[e].unwrap() / sum))
            .collect();
    }
    match mode {
        ProbFill::Uniform => edges.iter().map(|&e| (e, 1.0 / n)).collect(),
        ProbFill::Residual => {
            let residual = 1.0 - sum;
            if residual <= 1e-9 {
                // Authored mass already ≥ 1: unauthored get 0, authored renormalized.
                return edges
                    .iter()
                    .map(|&e| (e, authored[e].map(|p| p / sum).unwrap_or(0.0)))
                    .collect();
            }
            let each = residual / missing as f64;
            edges
                .iter()
                .map(|&e| (e, authored[e].unwrap_or(each)))
                .collect()
        }
    }
}
