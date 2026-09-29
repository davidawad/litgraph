// SPDX-License-Identifier: GPL-3.0-or-later
//! Ranking uncalibrated probabilities and durations by decision sensitivity,
//! so a user knows what to calibrate first — the same one-at-a-time
//! perturbation method `tornado` (`crate::algo::sweep`) uses for *authored*
//! inputs, run here over the inputs calibration hasn't reached yet.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::algo::{chain, mdp};
use crate::error::Result;
use crate::model::{Duration, Graph, NodeIx};
use crate::scenario::{Control, Scenario, View};

/// One row: what was perturbed and how much the answer swung. Reuses
/// `tornado`'s row shape (`input`/`low_value`/`high_value`/`v_low`/`v_high`/
/// `swing`/`policy_changes`) so the two are easy to compare.
pub use crate::algo::sweep::Sensitivity as Gap;

/// Uncalibrated probabilities and durations, each ranked by swing.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Gaps {
    /// V(start) under the scenario as given.
    pub base_value: f64,
    /// Uncalibrated draw probabilities, ranked by `|ΔV(start)|` when
    /// perturbed by ±`dp` (siblings rescaled by the usual fill logic).
    pub probabilities: Vec<Gap>,
    /// True if more candidates existed than `max_candidates` allowed scanning.
    pub probabilities_truncated: bool,
    /// Expected elapsed days to absorption under the scenario as given.
    pub base_elapsed_days: f64,
    /// Uncalibrated edge durations, ranked by `|Δ expected elapsed days|`
    /// when the duration is perturbed by ±`rel` (of its deadline-length
    /// fallback, or a fixed 0–30 day band absent one).
    pub durations: Vec<Gap>,
    /// True if more candidates existed than `max_candidates` allowed scanning.
    pub durations_truncated: bool,
}

fn expected_elapsed(g: &Graph, sc: &Scenario) -> Result<f64> {
    let v = View::new(g, sc)?;
    let sol = mdp::solve(&v, &mdp::SolveOptions::default())?;
    let elapsed = v.metric("elapsed")?;
    let c = chain::chain(&v, &sol, v.start, &[("elapsed".to_string(), elapsed)])?;
    Ok(c.expected.get("elapsed").copied().unwrap_or(0.0))
}

fn probability_gaps(
    g: &Graph,
    sc: &Scenario,
    base_v: &View<'_>,
    base_pol: &BTreeMap<NodeIx, usize>,
    dp: f64,
    max_candidates: usize,
) -> Result<(Vec<Gap>, bool)> {
    let eval = |sc2: &Scenario| -> Result<(f64, bool)> {
        let v = View::new(g, sc2)?;
        let s = mdp::solve(&v, &mdp::SolveOptions::default())?;
        let pol: BTreeMap<NodeIx, usize> = s.my_policy(&v).collect();
        Ok((s.value[v.start], &pol != base_pol))
    };
    let mut out = vec![];
    let mut truncated = false;
    for e in 0..g.edges.len() {
        if g.edges[e].probability.is_some() || g.edges[e].synthetic {
            continue;
        }
        let from = g.edges[e].from;
        if !base_v.active[e] || base_v.plan[from].control == Control::Terminal {
            continue;
        }
        let Some(p0) = base_v.prob[e] else { continue };
        // Skip binary-node mirror duplicates, as `tornado` does: perturbing
        // one side of a 2-way draw is the other side mirrored.
        let draws = &base_v.plan[from].draws;
        if draws.len() == 2 && draws[1].0 == e {
            continue;
        }
        if out.len() >= max_candidates {
            truncated = true;
            break;
        }
        let (lo, hi) = ((p0 - dp).max(0.0), (p0 + dp).min(1.0));
        let mut a = sc.clone();
        a.probabilities.insert(g.edges[e].id.clone(), lo);
        let mut b = sc.clone();
        b.probabilities.insert(g.edges[e].id.clone(), hi);
        let ((vl, cl), (vh, ch)) = (eval(&a)?, eval(&b)?);
        out.push(Gap {
            input: format!("p:{}", g.edges[e].id),
            low_value: lo,
            high_value: hi,
            v_low: vl,
            v_high: vh,
            swing: (vh - vl).abs(),
            policy_changes: cl || ch,
        });
    }
    out.sort_by(|a, b| b.swing.total_cmp(&a.swing));
    Ok((out, truncated))
}

fn duration_gaps(
    g: &Graph,
    sc: &Scenario,
    base_v: &View<'_>,
    rel: f64,
    max_candidates: usize,
) -> Result<(Vec<Gap>, bool)> {
    let mut out = vec![];
    let mut truncated = false;
    for e in 0..g.edges.len() {
        if g.edges[e].duration.is_some() || g.edges[e].synthetic || !base_v.active[e] {
            continue;
        }
        if out.len() >= max_candidates {
            truncated = true;
            break;
        }
        let dl = g.edges[e].deadline.as_ref().map_or(0.0, |d| d.length);
        let (lo, hi) = if dl > 0.0 {
            ((dl * (1.0 - rel)).max(0.0), dl * (1.0 + rel))
        } else {
            (0.0, 30.0)
        };
        let mut ga = g.clone();
        ga.edges[e].duration = Some(Duration {
            min: None,
            mode: lo,
            max: None,
        });
        let mut gb = g.clone();
        gb.edges[e].duration = Some(Duration {
            min: None,
            mode: hi,
            max: None,
        });
        let el = expected_elapsed(&ga, sc)?;
        let eh = expected_elapsed(&gb, sc)?;
        out.push(Gap {
            input: format!("duration:{}", g.edges[e].id),
            low_value: lo,
            high_value: hi,
            v_low: el,
            v_high: eh,
            swing: (eh - el).abs(),
            // Duration rarely flips the dollar-optimal policy by itself;
            // this row measures the elapsed-time swing, not V(start).
            policy_changes: false,
        });
    }
    out.sort_by(|a, b| b.swing.total_cmp(&a.swing));
    Ok((out, truncated))
}

/// Scan for uncalibrated probabilities and durations, ranked by decision
/// sensitivity. `dp` bounds the probability perturbation (as in `tornado`);
/// `rel` is the relative perturbation applied to an uncalibrated duration's
/// fallback (deadline length, else a fixed band); `max_candidates` caps how
/// many edges of each kind are actually solved for, so a large multi-pack
/// selection stays cheap.
///
/// # Errors
/// View, solve, or chain errors at the base scenario or any perturbation.
pub fn gaps(g: &Graph, sc: &Scenario, dp: f64, rel: f64, max_candidates: usize) -> Result<Gaps> {
    let base_v = View::new(g, sc)?;
    let base_sol = mdp::solve(&base_v, &mdp::SolveOptions::default())?;
    let base_value = base_sol.value[base_v.start];
    let base_pol: BTreeMap<NodeIx, usize> = base_sol.my_policy(&base_v).collect();

    let (probabilities, probabilities_truncated) =
        probability_gaps(g, sc, &base_v, &base_pol, dp, max_candidates)?;
    let base_elapsed_days = expected_elapsed(g, sc)?;
    let (durations, durations_truncated) = duration_gaps(g, sc, &base_v, rel, max_candidates)?;

    Ok(Gaps {
        base_value,
        probabilities,
        probabilities_truncated,
        base_elapsed_days,
        durations,
        durations_truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CompileOptions, LinkFile, Pack};

    fn demo_graph() -> Graph {
        let json = r#"{
            "schemaVersion": 2, "id": "demo", "title": "Demo", "startNodeId": "start",
            "nodes": [
                {"id": "start", "label": "Start"},
                {"id": "win", "label": "Win", "kind": "terminal", "payoff": 100.0},
                {"id": "lose", "label": "Lose", "kind": "terminal", "payoff": 0.0}
            ],
            "edges": [
                {"id": "to-win", "from": "start", "to": "win", "label": "win", "actor": "examiner"},
                {"id": "to-lose", "from": "start", "to": "lose", "label": "lose", "actor": "examiner"}
            ]
        }"#;
        let pack = Pack::from_json(json).unwrap();
        Graph::compile(&[pack], &LinkFile::default(), &CompileOptions::default()).unwrap()
    }

    #[test]
    fn gaps_reports_the_uncalibrated_edge() {
        let g = demo_graph();
        let sc = Scenario::default();
        let out = gaps(&g, &sc, 0.1, 0.25, 100).unwrap();
        assert!(out.probabilities.iter().all(|row| row.swing.is_finite()));
        assert!(out.durations.iter().all(|row| row.swing.is_finite()));
        assert!(!out.probabilities_truncated);
        assert!(!out.durations_truncated);
    }

    #[test]
    fn max_candidates_truncates_and_says_so() {
        let g = demo_graph();
        let sc = Scenario::default();
        // Two edges are candidates for both probability and duration gaps;
        // capping at 1 must mark truncation.
        let out = gaps(&g, &sc, 0.1, 0.25, 1).unwrap();
        assert!(out.probabilities.len() <= 1);
        assert!(out.durations.len() <= 1);
        // Two duration candidates exist (durations have no mirror-skip, unlike
        // probabilities), so capping at 1 must mark truncation.
        assert!(out.durations_truncated);
    }

    #[test]
    fn calibrated_edges_are_excluded_from_gaps() {
        let mut g = demo_graph();
        let e = g.edge("demo::to-win").unwrap();
        g.edges[e].probability = Some(0.6);
        g.edges[e].duration = Some(Duration {
            min: None,
            mode: 10.0,
            max: None,
        });
        let sc = Scenario::default();
        let out = gaps(&g, &sc, 0.1, 0.25, 100).unwrap();
        assert!(!out
            .probabilities
            .iter()
            .any(|row| row.input.contains("to-win")));
        assert!(!out.durations.iter().any(|row| row.input.contains("to-win")));
    }
}
