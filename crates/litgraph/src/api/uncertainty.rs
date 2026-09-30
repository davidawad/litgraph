// SPDX-License-Identifier: GPL-3.0-or-later
//! Uncertainty ops: `posterior`, `voi`, and `solve`'s nominal-vs-robust
//! report under `objective: robust`. See `docs/UNCERTAINTY.md`.

use serde_json::{json, Value};

use super::op::{Op, StudySpec};
use super::render::{choice_id, choice_label, node_ref, r};
use super::Warn;
use crate::algo::posterior::{self, Interval, PosteriorOptions};
use crate::algo::voi::{self, Study, VoiOptions};
use crate::algo::{mdp, robust};
use crate::error::{Error, Result};
use crate::model::NodeIx;
use crate::scenario::{Control, Objective, View};

type Out = Result<(Value, Vec<Warn>)>;

fn warns(ws: Vec<crate::scenario::Warning>) -> Vec<Warn> {
    ws.into_iter()
        .map(|w| Warn {
            code: w.code.into(),
            at: w.at,
            message: w.message,
        })
        .collect()
}

/// Estimated-prior warnings for `groups`, plus a note when the scenario's
/// objective or general-sum opponent is set aside for a risk-neutral solve.
fn uncertainty_warnings(v: &View, groups: &[usize], op: &str) -> Vec<Warn> {
    let mut out = warns(v.belief.estimated_warnings(v.g, groups, &format!("`{op}`")));
    if v.sc.objective != Objective::Expected || v.sc.opponent_objective.is_some() {
        out.push(Warn {
            code: "uncertainty-risk-neutral".into(),
            at: None,
            message: format!(
                "`{op}` values decisions risk-neutrally against a zero-sum opponent (objective `expected`); the scenario's objective / opponent_objective is not applied here"
            ),
        });
    }
    out
}

fn interval(i: &Interval) -> Value {
    json!({ "nominal": r(i.nominal), "mean": r(i.mean), "lo": r(i.lo), "hi": r(i.hi) })
}

/// Run `posterior` / `voi` against a resolved view.
///
/// # Errors
/// Unknown refs or any op error; `Invalid` for an op this module doesn't run.
pub(super) fn dispatch(v: &View, op: &Op) -> Out {
    let start_of = |from: Option<&String>| from.map_or(Ok(v.start), |f| v.g.node(f));
    match op {
        Op::Posterior {
            from,
            node,
            samples,
            seed,
            credibility,
        } => {
            let start = start_of(from.as_ref())?;
            let n = node.as_ref().map_or(Ok(start), |x| v.g.node(x))?;
            let o = PosteriorOptions {
                samples: *samples,
                seed: *seed,
                credibility: *credibility,
            };
            posterior_op(v, start, n, &o)
        }
        Op::Voi {
            from,
            samples,
            seed,
            top,
            max_nodes,
            studies,
        } => {
            let o = VoiOptions {
                samples: *samples,
                seed: *seed,
                top: *top,
                max_nodes: *max_nodes,
            };
            voi_op(v, start_of(from.as_ref())?, studies, &o)
        }
        other => Err(Error::Invalid(format!(
            "{} is not an uncertainty op",
            other.name()
        ))),
    }
}

fn posterior_op(v: &View, start: NodeIx, node: NodeIx, o: &PosteriorOptions) -> Out {
    let p = posterior::posterior(v, start, node, o)?;
    let options: Vec<Value> = p
        .options
        .iter()
        .map(|s| {
            json!({
                "edge": choice_id(v, s.edge),
                "label": choice_label(v, s.edge),
                "q": interval(&s.q),
                "p_optimal": r(s.p_optimal),
                "nominal_best": p.nominal_choice == Some(s.edge),
            })
        })
        .collect();
    let result = json!({
        "start": node_ref(v, start),
        "node": node_ref(v, node),
        "samples": o.samples,
        "credibility": o.credibility,
        "uncertain_nodes": p.uncertain.len(),
        "value": interval(&p.value),
        "options": options,
        "unconverged_samples": p.unconverged,
    });
    Ok((result, uncertainty_warnings(v, &p.uncertain, "posterior")))
}

fn study_of(v: &View, s: &StudySpec) -> Result<(Study, Option<f64>)> {
    let n = v.g.node(&s.node)?;
    let group = v.belief.group_at(n).ok_or_else(|| {
        Error::Invalid(format!(
            "voi: study node {} is not a chance draw under this scenario",
            v.g.nodes[n].id
        ))
    })?;
    let cost = match (s.cost, &s.cost_edge) {
        (Some(_), Some(_)) => {
            return Err(Error::Invalid(
                "voi: give a study `cost` or `cost_edge`, not both".into(),
            ))
        }
        (Some(c), None) => Some(c),
        (None, Some(e)) => {
            let c = v.cost[v.g.edge(e)?];
            if !c.is_finite() {
                return Err(Error::Invalid(format!(
                    "voi: cost_edge {e} is inactive under this scenario"
                )));
            }
            Some(c)
        }
        (None, None) => None,
    };
    Ok((Study { group, k: s.k }, cost))
}

fn voi_op(v: &View, start: NodeIx, specs: &[StudySpec], o: &VoiOptions) -> Out {
    let resolved: Vec<(Study, Option<f64>)> = specs
        .iter()
        .map(|s| study_of(v, s))
        .collect::<Result<_>>()?;
    let studies: Vec<Study> = resolved.iter().map(|x| x.0).collect();
    let out = voi::voi(v, start, &studies, o)?;
    let group_json = |gi: usize| {
        let grp = &v.belief.groups[gi];
        json!({
            "node": node_ref(v, grp.node),
            "kind": grp.kind,
            "outcomes": grp.edges.iter().map(|&e| v.g.edges[e].id.clone()).collect::<Vec<_>>(),
            "residual": grp.residual,
            "concentration": r(grp.concentration),
            "prior": grp.source,
        })
    };
    let est = |e: &voi::Estimate| json!({ "value": r(e.value), "std_error": r(e.std_error) });
    let nodes: Vec<Value> = out
        .nodes
        .iter()
        .map(|row| {
            let mut j = group_json(row.group);
            j["evpi_outcome"] = row.evpi_outcome.map_or(Value::Null, r);
            j["evppi"] = row.evppi.as_ref().map_or(Value::Null, est);
            j
        })
        .collect();
    let studies: Vec<Value> = specs
        .iter()
        .zip(&resolved)
        .zip(&out.studies)
        .map(|((spec, (st, cost)), e)| {
            let row = out.nodes.iter().find(|n| n.group == st.group);
            let mut j = group_json(st.group);
            j["label"] = json!(spec.label);
            j["k"] = json!(st.k);
            j["evsi"] = est(e);
            j["evpi_outcome"] = row.and_then(|n| n.evpi_outcome).map_or(Value::Null, r);
            j["cost"] = cost.map_or(Value::Null, r);
            j["net"] = cost.map_or(Value::Null, |c| r(e.value - c));
            j["worth_paying"] = cost.map_or(Value::Null, |c| json!(e.value > c));
            j
        })
        .collect();
    let screened: Vec<usize> = out.nodes.iter().map(|n| n.group).collect();
    let result = json!({
        "start": node_ref(v, start),
        "value": r(out.value),
        "samples": o.samples,
        "evpi_total": est(&out.evpi_total),
        "nodes": nodes,
        "nodes_truncated": out.truncated,
        "studies": studies,
        "unconverged_solves": out.unconverged,
    });
    Ok((result, uncertainty_warnings(v, &screened, "voi")))
}

/// `solve`'s extra block under `objective: robust`: the nominal (expected,
/// posterior-mean) value beside the robust one, the ambiguity radii, and
/// every reachable decision of ours whose choice differs.
///
/// # Errors
/// A solve error, or an invalid robust credibility/radius.
pub(super) fn robust_report(v: &View, robust_sol: &mdp::Solution, start: NodeIx) -> Result<Value> {
    let Objective::Robust {
        credibility,
        radius,
        samples,
        seed,
    } = v.sc.objective
    else {
        return Ok(Value::Null);
    };
    let nominal_view = posterior::risk_neutral(v);
    let nominal = mdp::solve(&nominal_view, &mdp::SolveOptions::default())?;
    let radii = robust::radii(v, credibility, radius, samples, seed)?;
    let ambiguous: Vec<f64> = radii.iter().copied().filter(|&x| x > 0.0).collect();
    let live = voi::reachable(v, start);
    let changes: Vec<Value> = (0..v.g.nodes.len())
        .filter(|&n| live[n] && v.plan[n].control == Control::Me)
        .filter_map(|n| {
            let (a, b) = (nominal.choice.get(&n)?, robust_sol.choice.get(&n)?);
            (a != b).then(|| {
                json!({
                    "node": node_ref(v, n),
                    "nominal": { "edge": choice_id(v, *a), "label": choice_label(v, *a) },
                    "robust": { "edge": choice_id(v, *b), "label": choice_label(v, *b) },
                })
            })
        })
        .collect();
    Ok(json!({
        "credibility": credibility,
        "radius": radius,
        "nominal_value": r(nominal.value[start]),
        "robust_value": r(robust_sol.value[start]),
        "price_of_robustness": r(nominal.value[start] - robust_sol.value[start]),
        "ambiguous_nodes": ambiguous.len(),
        "max_radius": r(ambiguous.iter().copied().fold(0.0, f64::max)),
        "policy_changes": changes,
    }))
}
