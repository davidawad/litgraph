// SPDX-License-Identifier: GPL-3.0-or-later
//! [`View`]: a scenario resolved against a graph.

use std::collections::{BTreeMap, VecDeque};

use super::plan::PlanInputs;
use super::{resolve_spec, NodePlan, Scenario, Warning};
use crate::error::{Error, Result};
use crate::expr;
use crate::metrics::{self, EdgeEnv, TerminalEnv};
use crate::model::{Graph, NodeIx, PayoffSource, Role};

/// A scenario resolved into dense arrays. Every algorithm consumes a view.
pub struct View<'g> {
    /// The compiled graph.
    pub g: &'g Graph,
    /// The scenario this view resolves.
    pub sc: Scenario,
    /// Parameters after defaults.
    pub params: BTreeMap<String, f64>,
    /// Edge survives masks/removals.
    pub active: Vec<bool>,
    /// Role of each edge under the scenario's perspective.
    pub role: Vec<Role>,
    /// Probability of each edge within its node's draw (None for pure choices).
    pub prob: Vec<Option<f64>>,
    /// Payoff per node after overrides.
    pub payoff: Vec<f64>,
    /// Terminal utility per node (0 off-terminal).
    pub utility: Vec<f64>,
    /// Scenario cost metric per edge (NaN if inactive).
    pub cost: Vec<f64>,
    /// Expected elapsed days per edge.
    pub elapsed: Vec<f64>,
    /// Who acts at each node.
    pub plan: Vec<NodePlan>,
    /// Unweighted steps to the nearest terminal over active edges (`u32::MAX` if none).
    pub dist_to_terminal: Vec<u32>,
    /// Forced choices.
    pub forced: BTreeMap<NodeIx, usize>,
    /// Start node.
    pub start: NodeIx,
    /// Everything the engine had to assume.
    pub warnings: Vec<Warning>,
}

fn params_of(sc: &Scenario) -> BTreeMap<String, f64> {
    let mut params = metrics::default_params();
    params.extend(sc.params.clone());
    if !sc.params.contains_key("opp_rate") {
        let rate = params.get("rate").copied().unwrap_or_default();
        params.insert("opp_rate".into(), rate);
    }
    params
}

fn edge_env<'a>(g: &'a Graph, e: usize, role: &[Role], p: f64, params: &'a BTreeMap<String, f64>, payoff: &[f64]) -> EdgeEnv<'a> {
    EdgeEnv { g, e, role: role[e], p, params, to_payoff: payoff[g.edges[e].to] }
}

/// Removals, then the mask expression.
fn active_edges(g: &Graph, sc: &Scenario, role: &[Role], params: &BTreeMap<String, f64>, payoff: &[f64]) -> Result<Vec<bool>> {
    let mut active = vec![true; g.edges.len()];
    for r in &sc.remove_edges {
        active[g.edge(r)?] = false;
    }
    if let Some(src) = &sc.mask {
        let m = expr::parse(src)?;
        for (e, a) in active.iter_mut().enumerate() {
            let env = edge_env(g, e, role, g.edges[e].probability.unwrap_or(1.0), params, payoff);
            if m.eval(&env)? == 0.0 {
                *a = false;
            }
        }
    }
    Ok(active)
}

/// Authored probabilities after scenario overrides (siblings rescaled) and `probability_fn`.
fn probabilities(g: &Graph, sc: &Scenario, v: &ViewParts<'_>) -> Result<Vec<Option<f64>>> {
    let mut authored: Vec<Option<f64>> = g.edges.iter().map(|e| e.probability).collect();
    for (r, &p) in &sc.probabilities {
        let e = g.edge(r)?;
        if !(0.0..=1.0).contains(&p) {
            return Err(Error::Invalid(format!("probability for {r} must be in [0,1]")));
        }
        let others: Vec<usize> = g.out[g.edges[e].from].iter().copied().filter(|&o| o != e && authored[o].is_some()).collect();
        let rest: f64 = others.iter().filter_map(|&o| authored[o]).sum();
        let target_rest = (rest + authored[e].unwrap_or(0.0) - p).max(0.0);
        authored[e] = Some(p);
        if rest > 0.0 {
            for o in others {
                authored[o] = authored[o].map(|x| x * target_rest / rest);
            }
        }
    }
    if let Some(src) = &sc.probability_fn {
        let ex = expr::parse(src)?;
        for e in (0..g.edges.len()).filter(|&e| v.active[e] && v.role[e] != Role::Me) {
            let env = edge_env(g, e, v.role, authored[e].unwrap_or(f64::NAN), v.params, v.payoff);
            let x = ex.eval(&env).map_err(|err| Error::Expr(format!("probability_fn on {}: {err}", g.edges[e].id)))?;
            if x.is_finite() {
                authored[e] = Some(x.clamp(0.0, 1.0));
            }
        }
    }
    Ok(authored)
}

struct ViewParts<'a> {
    active: &'a [bool],
    role: &'a [Role],
    params: &'a BTreeMap<String, f64>,
    payoff: &'a [f64],
}

fn dist_to_terminal(g: &Graph, active: &[bool]) -> Vec<u32> {
    let mut dist = vec![u32::MAX; g.nodes.len()];
    let mut queue: VecDeque<NodeIx> = g.terminals().collect();
    for &t in &queue {
        dist[t] = 0;
    }
    while let Some(u) = queue.pop_front() {
        for &e in &g.inc[u] {
            let w = g.edges[e].from;
            if active[e] && dist[w] == u32::MAX {
                dist[w] = dist[u] + 1;
                queue.push_back(w);
            }
        }
    }
    dist
}

impl<'g> View<'g> {
    /// Resolve `sc` against `g`.
    ///
    /// # Errors
    /// Unknown node/edge refs, bad expressions, out-of-range probabilities.
    pub fn new(g: &'g Graph, sc: &Scenario) -> Result<View<'g>> {
        let params = params_of(sc);
        let role: Vec<Role> = (0..g.edges.len())
            .map(|e| sc.perspective.get(&g.edges[e].actor).copied().unwrap_or_else(|| g.base_role(e)))
            .collect();
        let mut payoff: Vec<f64> = g.nodes.iter().map(|n| n.payoff).collect();
        for (r, v) in &sc.payoffs {
            payoff[g.node(r)?] = *v;
        }
        let active = active_edges(g, sc, &role, &params, &payoff)?;
        let parts = ViewParts { active: &active, role: &role, params: &params, payoff: &payoff };
        let authored = probabilities(g, sc, &parts)?;
        let mut forced = BTreeMap::new();
        for (n, e) in &sc.policy {
            let ni = g.node(n)?;
            forced.insert(ni, g.edge_at(ni, e)?);
        }
        let plans = PlanInputs { g, sc, active: &active, role: &role, authored: &authored }.build();
        let start = sc.start.as_deref().map_or(Ok(g.start), |s| g.node(s))?;
        let mut view = View {
            g,
            sc: sc.clone(),
            params,
            dist_to_terminal: dist_to_terminal(g, &active),
            active,
            role,
            prob: plans.prob,
            payoff,
            utility: vec![0.0; g.nodes.len()],
            cost: vec![],
            elapsed: vec![],
            plan: plans.plan,
            forced,
            start,
            warnings: plans.warnings,
        };
        view.utility = view.utilities()?;
        let cost_spec = sc.cost.clone().unwrap_or_else(|| "dollars".into());
        view.cost = view.metric(&cost_spec)?;
        view.elapsed = view.metric("elapsed")?;
        if view.cost.iter().zip(&view.active).any(|(c, a)| *a && *c < 0.0) {
            view.warnings.push(Warning {
                code: "negative-cost",
                at: None,
                message: format!("cost metric `{cost_spec}` is negative on some edges (treated as a reward)"),
            });
        }
        Ok(view)
    }

    fn utilities(&mut self) -> Result<Vec<f64>> {
        let spec = self.sc.utility.clone().unwrap_or_else(|| "ev".into());
        let src = resolve_spec(&spec, &self.sc.utilities, metrics::UTILITIES).to_string();
        let ex = expr::parse(&src)?;
        let overridden: Vec<NodeIx> = self.sc.payoffs.keys().filter_map(|k| self.g.node(k).ok()).collect();
        let mut out = vec![0.0; self.g.nodes.len()];
        let mut guessed = 0;
        for n in self.g.terminals() {
            let env = TerminalEnv { g: self.g, n, payoff: self.payoff[n], params: &self.params };
            out[n] = ex.eval(&env).map_err(|e| Error::Expr(format!("utility `{src}` at {}: {e}", self.g.nodes[n].id)))?;
            if self.g.nodes[n].payoff_source != PayoffSource::Authored && !overridden.contains(&n) {
                guessed += 1;
            }
        }
        if guessed > 0 {
            self.warnings.push(Warning {
                code: "payoff-not-authored",
                at: None,
                message: format!("{guessed} terminal(s) use a heuristic or zero payoff; override with scenario.payoffs or author `payoff` in the pack"),
            });
        }
        Ok(out)
    }

    /// Evaluate a metric (name or expression) on every edge; inactive edges get NaN.
    ///
    /// # Errors
    /// Parse or evaluation errors, naming the edge.
    pub fn metric(&self, spec: &str) -> Result<Vec<f64>> {
        let src = resolve_spec(spec, &self.sc.metrics, metrics::METRICS);
        let ex = expr::parse(src)?;
        (0..self.g.edges.len())
            .map(|e| {
                if !self.active[e] {
                    return Ok(f64::NAN);
                }
                ex.eval(&self.edge_env(e))
                    .map_err(|err| Error::Expr(format!("metric `{src}` on edge {}: {err}", self.g.edges[e].id)))
            })
            .collect()
    }

    /// Evaluate a terminal expression on every node; non-terminals get NaN.
    ///
    /// # Errors
    /// Parse or evaluation errors.
    pub fn terminal_metric(&self, spec: &str) -> Result<Vec<f64>> {
        let src = resolve_spec(spec, &self.sc.utilities, metrics::UTILITIES);
        let ex = expr::parse(src)?;
        (0..self.g.nodes.len())
            .map(|n| {
                if !self.g.nodes[n].is_terminal() {
                    return Ok(f64::NAN);
                }
                ex.eval(&TerminalEnv { g: self.g, n, payoff: self.payoff[n], params: &self.params })
            })
            .collect()
    }

    /// The expression environment of edge `e` under this view.
    #[must_use]
    pub fn edge_env(&self, e: usize) -> EdgeEnv<'_> {
        edge_env(self.g, e, &self.role, self.prob[e].unwrap_or(1.0), &self.params, &self.payoff)
    }

    /// Active out-edges of a node.
    pub fn outs(&self, n: NodeIx) -> impl Iterator<Item = usize> + '_ {
        self.g.out[n].iter().copied().filter(move |&e| self.active[e])
    }
}
