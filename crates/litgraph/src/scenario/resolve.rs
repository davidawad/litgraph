// SPDX-License-Identifier: GPL-3.0-or-later
//! Pure resolution helpers used to build a [`super::View`]: default
//! parameters, the edge expression environment, which edges survive masks
//! and removals, authored probabilities after scenario overrides, and
//! unweighted distance to the nearest terminal. `View::new` composes these;
//! none of them read or write a `View` directly, so they're kept separate
//! from its query methods.

use std::collections::{BTreeMap, VecDeque};

use super::Scenario;
use crate::error::{Error, Result};
use crate::expr;
use crate::metrics::{self, EdgeEnv};
use crate::model::{Graph, NodeIx, Role};

/// Default scenario parameters, plus `opp_rate` defaulted to `rate` when the
/// scenario doesn't set it explicitly.
pub(super) fn params_of(sc: &Scenario) -> BTreeMap<String, f64> {
    let mut params = metrics::default_params();
    params.extend(sc.params.clone());
    if !sc.params.contains_key("opp_rate") {
        let rate = params.get("rate").copied().unwrap_or_default();
        params.insert("opp_rate".into(), rate);
    }
    params
}

pub(super) fn edge_env<'a>(
    g: &'a Graph,
    e: usize,
    role: &[Role],
    p: f64,
    params: &'a BTreeMap<String, f64>,
    payoff: &[f64],
) -> EdgeEnv<'a> {
    EdgeEnv {
        g,
        e,
        role: role[e],
        p,
        params,
        to_payoff: payoff[g.edges[e].to],
    }
}

/// Removals, then the mask expression.
pub(super) fn active_edges(
    g: &Graph,
    sc: &Scenario,
    role: &[Role],
    params: &BTreeMap<String, f64>,
    payoff: &[f64],
) -> Result<Vec<bool>> {
    let mut active = vec![true; g.edges.len()];
    for r in &sc.remove_edges {
        active[g.edge(r)?] = false;
    }
    if let Some(src) = &sc.mask {
        let m = expr::parse(src)?;
        for (e, a) in active.iter_mut().enumerate() {
            let env = edge_env(
                g,
                e,
                role,
                g.edges[e].probability.unwrap_or(1.0),
                params,
                payoff,
            );
            if m.eval(&env)? == 0.0 {
                *a = false;
            }
        }
    }
    Ok(active)
}

/// Inputs to [`probabilities`], grouped to keep its signature short.
pub(super) struct ViewParts<'a> {
    pub active: &'a [bool],
    pub role: &'a [Role],
    pub params: &'a BTreeMap<String, f64>,
    pub payoff: &'a [f64],
}

/// Authored probabilities after scenario overrides (siblings rescaled) and `probability_fn`.
///
/// `overrides` is `scenario.probabilities` plus every `scenario.facts` entry
/// resolved to its edge's canonical id at probability 1.0 (see
/// [`super::View::new`]) — facts are "force this edge" sugar over the same
/// rescale-siblings mechanism, not a separate code path.
pub(super) fn probabilities(
    g: &Graph,
    sc: &Scenario,
    v: &ViewParts<'_>,
    overrides: &BTreeMap<String, f64>,
) -> Result<Vec<Option<f64>>> {
    let mut authored: Vec<Option<f64>> = g.edges.iter().map(|e| e.probability).collect();
    for (r, &p) in overrides {
        let e = g.edge(r)?;
        if !(0.0..=1.0).contains(&p) {
            return Err(Error::Invalid(format!(
                "probability for {r} must be in [0,1]"
            )));
        }
        let others: Vec<usize> = g.out[g.edges[e].from]
            .iter()
            .copied()
            .filter(|&o| o != e && authored[o].is_some())
            .collect();
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
            let env = edge_env(
                g,
                e,
                v.role,
                authored[e].unwrap_or(f64::NAN),
                v.params,
                v.payoff,
            );
            let x = ex.eval(&env).map_err(|err| {
                Error::Expr(format!("probability_fn on {}: {err}", g.edges[e].id))
            })?;
            if x.is_finite() {
                authored[e] = Some(x.clamp(0.0, 1.0));
            }
        }
    }
    Ok(authored)
}

/// Unweighted steps to the nearest terminal over active edges (`u32::MAX` if none).
pub(super) fn dist_to_terminal(g: &Graph, active: &[bool]) -> Vec<u32> {
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
