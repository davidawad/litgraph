// SPDX-License-Identifier: GPL-3.0-or-later
//! [`View`]: a scenario resolved against a graph.

use std::collections::{BTreeMap, HashSet};

use super::plan::PlanInputs;
use super::resolve::{
    active_edges, dist_to_terminal, edge_env, params_of, probabilities, ViewParts,
};
use super::{limits, resolve_spec, NodePlan, Scenario, Warning};
use crate::error::{Error, Result};
use crate::expr::{self, Expr};
use crate::metrics::{self, EdgeEnv, PathVars, TerminalEnv};
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
    /// The resolved utility expression, for re-evaluation with real
    /// path-dependent variables (`simulate` uses this; `utility` above is the
    /// Markov (path vars = 0) value `solve`/`chain` use).
    pub utility_expr: Expr,
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

/// `scenario.facts` is sugar over `scenario.probabilities`: each {node ref:
/// edge ref} resolves to that edge's canonical id forced to 1.0 (siblings
/// fall to 0 via the usual rescale, in [`probabilities`]). The returned
/// `HashSet` records which nodes had an entry, so a `fact`-tagged node
/// without one can warn instead of silently taking a heuristic/uniform
/// fallback.
fn fact_overrides(g: &Graph, sc: &Scenario) -> Result<(BTreeMap<String, f64>, HashSet<NodeIx>)> {
    let mut overrides = sc.probabilities.clone();
    let mut fact_nodes: HashSet<NodeIx> = HashSet::new();
    for (node_ref, edge_ref) in &sc.facts {
        let ni = g.node(node_ref)?;
        let ei = g.edge_at(ni, edge_ref)?;
        fact_nodes.insert(ni);
        overrides.insert(g.edges[ei].id.clone(), 1.0);
    }
    Ok((overrides, fact_nodes))
}

/// `scenario.facts` is sugar over `scenario.probabilities`: each `{node ref:
/// edge ref}` resolves to that edge's canonical id forced to 1.0 (siblings
/// fall to 0 via the usual rescale). Returns the probability overrides plus
/// which nodes had an entry, so a `fact`-tagged node without one can warn
/// instead of silently taking a heuristic/uniform fallback.
///
/// A fact is authored once for the pack node it describes, not once per
/// state-flag history that node can be reached under (same reasoning as
/// calibration's `edge_family`, `docs/PACK_SCHEMA.md#state-flags`): it
/// applies to every flagged copy of `node_ref` (`Graph::node_family`),
/// forcing each copy's own instantiation of `edge_ref`'s edge
/// (`Graph::edge_family`, matched by `base_id` rather than re-resolving
/// `edge_ref` per copy, since a flagged edge's id carries a `{flag}` suffix
/// the author's string won't contain).
fn fact_overrides(g: &Graph, sc: &Scenario) -> Result<(BTreeMap<String, f64>, HashSet<NodeIx>)> {
    let mut overrides = sc.probabilities.clone();
    let mut fact_nodes: HashSet<NodeIx> = HashSet::new();
    for (node_ref, edge_ref) in &sc.facts {
        let ni = g.node(node_ref)?;
        let ei = g.edge_at(ni, edge_ref)?;
        fact_nodes.extend(g.node_family(ni));
        for ej in g.edge_family(ei) {
            overrides.insert(g.edges[ej].id.clone(), 1.0);
        }
    }
    Ok((overrides, fact_nodes))
}

impl<'g> View<'g> {
    /// Resolve `sc` against `g`.
    ///
    /// ```
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
    /// // The lone terminal's utility is its authored payoff (the default `ev` utility).
    /// assert_eq!(v.utility[g.node("demo::end").unwrap()], 100.0);
    /// ```
    ///
    /// # Errors
    /// Unknown node/edge refs, bad expressions, out-of-range probabilities.
    pub fn new(g: &'g Graph, sc: &Scenario) -> Result<View<'g>> {
        let params = params_of(sc);
        let role: Vec<Role> = (0..g.edges.len())
            .map(|e| {
                sc.perspective
                    .get(&g.edges[e].actor)
                    .copied()
                    .unwrap_or_else(|| g.base_role(e))
            })
            .collect();
        let mut payoff: Vec<f64> = g.nodes.iter().map(|n| n.payoff).collect();
        for (r, v) in &sc.payoffs {
            payoff[g.node(r)?] = *v;
        }
        let active = active_edges(g, sc, &role, &params, &payoff)?;
        let parts = ViewParts {
            active: &active,
            role: &role,
            params: &params,
            payoff: &payoff,
        };
        let (overrides, fact_nodes) = fact_overrides(g, sc)?;
        let authored = probabilities(g, sc, &parts, &overrides)?;
        let mut forced = BTreeMap::new();
        for (n, e) in &sc.policy {
            let ni = g.node(n)?;
            forced.insert(ni, g.edge_at(ni, e)?);
        }
        let plans = PlanInputs {
            g,
            sc,
            active: &active,
            role: &role,
            authored: &authored,
            fact_nodes: &fact_nodes,
        }
        .build();
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
            utility_expr: Expr::Num(0.0),
            cost: vec![],
            elapsed: vec![],
            plan: plans.plan,
            forced,
            start,
            warnings: plans.warnings,
        };
        view.finish(sc)?;
        Ok(view)
    }

    /// Resolves utility/cost/elapsed and the combination warnings that need
    /// the fully-built view (`utilities()` for `path-variable-in-markov`,
    /// `fee_shift`/`cvar`/`opponent_objective` limits). Split out of `new`
    /// purely to keep it short; not meaningful to call on its own.
    fn finish(&mut self, sc: &Scenario) -> Result<()> {
        let (utility, utility_expr) = self.utilities()?;
        self.utility = utility;
        self.utility_expr = utility_expr;
        self.warnings.extend(limits::fee_shift_path_vars(sc)?);
        let cost_spec = sc.cost.clone().unwrap_or_else(|| "dollars".into());
        self.cost = self.metric(&cost_spec)?;
        self.elapsed = self.metric("elapsed")?;
        if self
            .cost
            .iter()
            .zip(&self.active)
            .any(|(c, a)| *a && *c < 0.0)
        {
            self.warnings.push(Warning {
                code: "negative-cost",
                at: None,
                message: format!(
                    "cost metric `{cost_spec}` is negative on some edges (treated as a reward)"
                ),
            });
        }
        self.warnings.extend(limits::cvar_limits(sc));
        self.warnings.extend(limits::opponent_objective_limits(sc));
        Ok(())
    }

    fn utilities(&mut self) -> Result<(Vec<f64>, Expr)> {
        let spec = self.sc.utility.clone().unwrap_or_else(|| "ev".into());
        let src = resolve_spec(&spec, &self.sc.utilities, metrics::UTILITIES).to_string();
        let ex = expr::parse(&src)?;
        self.warnings.extend(limits::path_var_warning_for(
            &format!("utility `{src}`"),
            &ex,
        ));
        let overridden: Vec<NodeIx> = self
            .sc
            .payoffs
            .keys()
            .filter_map(|k| self.g.node(k).ok())
            .collect();
        let mut out = vec![0.0; self.g.nodes.len()];
        let mut guessed = 0;
        for n in self.g.terminals() {
            let env = TerminalEnv {
                g: self.g,
                n,
                payoff: self.payoff[n],
                params: &self.params,
                path: PathVars::default(),
            };
            out[n] = ex.eval(&env).map_err(|e| {
                Error::Expr(format!("utility `{src}` at {}: {e}", self.g.nodes[n].id))
            })?;
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
        Ok((out, ex))
    }

    /// Evaluate the resolved utility expression at terminal `n` with explicit
    /// path-dependent variables. `solve`/`chain` use the precomputed
    /// `utility` field (path vars default to 0); `simulate` calls this with
    /// the actual accumulated `spent`/`elapsed_total`/`steps` for the sampled
    /// trajectory that reached `n`.
    ///
    /// # Errors
    /// Expression evaluation errors (should not occur: the same expression
    /// evaluated cleanly at every terminal when the view was constructed).
    pub fn utility_at(&self, n: NodeIx, path: PathVars) -> Result<f64> {
        self.utility_expr
            .eval(&TerminalEnv {
                g: self.g,
                n,
                payoff: self.payoff[n],
                params: &self.params,
                path,
            })
            .map_err(|e| {
                Error::Expr(format!(
                    "utility at {} with path vars: {e}",
                    self.g.nodes[n].id
                ))
            })
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
                ex.eval(&self.edge_env(e)).map_err(|err| {
                    Error::Expr(format!(
                        "metric `{src}` on edge {}: {err}",
                        self.g.edges[e].id
                    ))
                })
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
                ex.eval(&TerminalEnv {
                    g: self.g,
                    n,
                    payoff: self.payoff[n],
                    params: &self.params,
                    path: PathVars::default(),
                })
            })
            .collect()
    }

    /// The expression environment of edge `e` under this view.
    #[must_use]
    pub fn edge_env(&self, e: usize) -> EdgeEnv<'_> {
        edge_env(
            self.g,
            e,
            &self.role,
            self.prob[e].unwrap_or(1.0),
            &self.params,
            &self.payoff,
        )
    }

    /// Active out-edges of a node.
    pub fn outs(&self, n: NodeIx) -> impl Iterator<Item = usize> + '_ {
        self.g.out[n]
            .iter()
            .copied()
            .filter(move |&e| self.active[e])
    }
}

#[cfg(test)]
#[path = "view_tests.rs"]
mod tests;
