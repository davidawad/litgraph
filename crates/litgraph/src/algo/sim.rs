// SPDX-License-Identifier: GPL-3.0-or-later
//! Monte Carlo simulation under a policy. The closed-form chain gives means;
//! simulation gives the *distribution*: percentiles, downside (CVaR), P(loss),
//! and path-dependent quantities (fee recovery on actual spend, sampled
//! durations) the Markov closed form cannot express.

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::Serialize;
use std::collections::BTreeMap;

use crate::algo::chain::step_dist;
use crate::algo::mdp::Solution;
use crate::error::Result;
use crate::expr;
use crate::metrics::TerminalEnv;
use crate::model::{NodeIx, Role};
use crate::scenario::{Control, FeeShift, View};

/// Summary statistics of a sampled distribution.
#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    /// Sample mean.
    pub mean: f64,
    /// Sample standard deviation.
    pub std: f64,
    /// Minimum (p0).
    pub min: f64,
    /// 5th percentile.
    pub p05: f64,
    /// 25th percentile.
    pub p25: f64,
    /// Median.
    pub p50: f64,
    /// 75th percentile.
    pub p75: f64,
    /// 95th percentile.
    pub p95: f64,
    /// Maximum (p100).
    pub max: f64,
}

/// Result of Monte Carlo simulation under a fixed policy.
#[derive(Debug, Clone, Serialize)]
pub struct SimResult {
    /// Number of runs simulated.
    pub runs: usize,
    /// RNG seed used (simulation is deterministic given seed + policy).
    pub seed: u64,
    /// Net outcome per run: utility(terminal) − our cost + fee recovery.
    pub net: Summary,
    /// Mean of the worst `alpha` fraction of net outcomes.
    pub cvar: f64,
    /// The tail fraction `cvar` was computed over.
    pub alpha: f64,
    /// Fraction of runs with a negative net outcome.
    pub p_loss: f64,
    /// Distribution summary for every requested metric, by name.
    pub metrics: BTreeMap<String, Summary>,
    /// terminal -> frequency.
    pub terminals: Vec<(NodeIx, f64)>,
    /// Runs that hit the step cap without terminating.
    pub truncated: usize,
    /// A few sample trajectories (edge sequences).
    pub samples: Vec<Vec<usize>>,
}

/// Options controlling a [`simulate`] run.
pub struct SimOptions {
    /// Number of runs to simulate.
    pub runs: usize,
    /// RNG seed.
    pub seed: u64,
    /// Tail fraction for `CVaR` (e.g. 0.1 = worst 10%).
    pub alpha: f64,
    /// Per-run step cap; runs that exceed it are marked truncated.
    pub max_steps: usize,
    /// Sample elapsed time from each edge's triangular (min, mode, max) duration.
    pub sample_durations: bool,
    /// Number of sample trajectories to retain (for inspection).
    pub keep_samples: usize,
}

fn summarize(mut xs: Vec<f64>) -> Summary {
    xs.sort_by(f64::total_cmp);
    let n = xs.len().max(1) as f64;
    let mean = xs.iter().sum::<f64>() / n;
    let var = xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
    let q = |p: f64| {
        if xs.is_empty() {
            f64::NAN
        } else {
            // `p` is a percentile in [0, 1] and `xs` is non-empty here, so
            // the rounded index is in `[0, xs.len() - 1]`: never negative,
            // never truncated in a way that escapes that range.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let ix = ((xs.len() - 1) as f64 * p).round() as usize;
            xs[ix]
        }
    };
    Summary {
        mean,
        std: var.sqrt(),
        min: q(0.0),
        p05: q(0.05),
        p25: q(0.25),
        p50: q(0.5),
        p75: q(0.75),
        p95: q(0.95),
        max: q(1.0),
    }
}

fn triangular(rng: &mut ChaCha8Rng, a: f64, c: f64, b: f64) -> f64 {
    if b <= a {
        return c;
    }
    let u: f64 = rng.gen();
    let f = (c - a) / (b - a);
    if u < f {
        a + ((b - a) * (c - a) * u).sqrt()
    } else {
        b - ((b - a) * (b - c) * (1.0 - u)).sqrt()
    }
}

/// Outcome of one simulated trajectory.
struct RunOutcome {
    /// utility(terminal) − our cost + fee recovery (0 if truncated before a terminal).
    net: f64,
    /// Per-metric total along the trajectory, parallel to the caller's `metrics`.
    per_metric: Vec<f64>,
    /// False if the run hit `max_steps` without reaching a terminal/sink.
    ended: bool,
    /// The terminal reached, if any (sinks and truncated runs have none).
    terminal: Option<NodeIx>,
    /// The edge sequence, if this run's trajectory is being kept.
    traj: Option<Vec<usize>>,
}

/// Simulates one trajectory from `start` under the precomputed step
/// distributions `dists`, sampling every metric in `metrics`.
#[allow(clippy::too_many_arguments)]
fn simulate_run(
    v: &View,
    start: NodeIx,
    dists: &[Vec<(usize, f64)>],
    fee: Option<&FeeShift>,
    elig: &[bool],
    metrics: &[(String, Vec<f64>)],
    elapsed_ix: Option<usize>,
    o: &SimOptions,
    rng: &mut ChaCha8Rng,
    keep_sample: bool,
) -> RunOutcome {
    let mut u = start;
    let mut spent = 0.0;
    let mut my_spent = 0.0;
    let mut acc = vec![0.0; metrics.len()];
    let mut traj = vec![];
    let mut steps = 0;
    let mut ended = true;
    while !matches!(v.plan[u].control, Control::Terminal | Control::Sink) {
        let d = &dists[u];
        if d.is_empty() || steps >= o.max_steps {
            ended = false;
            break;
        }
        let total: f64 = d.iter().map(|x| x.1).sum();
        let mut r = rng.gen::<f64>() * total;
        let mut pick = d[d.len() - 1].0;
        for &(e, p) in d {
            if r < p {
                pick = e;
                break;
            }
            r -= p;
        }
        spent += v.cost[pick];
        if v.role[pick] == Role::Me {
            my_spent += v.cost[pick];
        }
        for (i, (_, vals)) in metrics.iter().enumerate() {
            let mut x = vals[pick];
            if Some(i) == elapsed_ix && o.sample_durations {
                if let Some(du) = &v.g.edges[pick].duration {
                    x = triangular(rng, du.min.unwrap_or(du.mode), du.mode, du.max.unwrap_or(du.mode));
                }
            }
            if x.is_finite() {
                acc[i] += x;
            }
        }
        if keep_sample {
            traj.push(pick);
        }
        u = v.g.edges[pick].to;
        steps += 1;
    }
    let recovery = match fee {
        Some(f) if elig[u] => f.fraction * my_spent,
        _ => 0.0,
    };
    let terminal = (v.plan[u].control == Control::Terminal).then_some(u);
    let term_value = terminal.map_or(0.0, |t| v.utility[t]);
    RunOutcome {
        net: term_value - spent + recovery,
        per_metric: acc,
        ended,
        terminal,
        traj: keep_sample.then_some(traj),
    }
}

/// Builds the fee-eligibility indicator per node (true only for
/// fee-eligible terminals), if the scenario has a `fee_shift`.
fn fee_eligibility(v: &View, fee: Option<&FeeShift>) -> Result<Vec<bool>> {
    let Some(f) = fee else {
        return Ok(vec![false; v.g.nodes.len()]);
    };
    let ex = expr::parse(f.eligible.as_deref().unwrap_or("tag('fee-eligible')"))?;
    (0..v.g.nodes.len())
        .map(|n| {
            Ok(v.g.nodes[n].is_terminal()
                && ex.eval(&TerminalEnv { g: v.g, n, payoff: v.payoff[n], params: &v.params })? != 0.0)
        })
        .collect()
}

/// Monte Carlo-simulates `o.runs` trajectories under policy `sol` from
/// `start`, sampling the given `metrics` alongside net outcome.
///
/// # Errors
/// Propagates any error evaluating the fee-eligibility expression.
pub fn simulate(
    v: &View,
    sol: &Solution,
    start: NodeIx,
    metrics: &[(String, Vec<f64>)],
    o: &SimOptions,
) -> Result<SimResult> {
    let mut rng = ChaCha8Rng::seed_from_u64(o.seed);
    let dists: Vec<Vec<(usize, f64)>> = (0..v.g.nodes.len())
        .map(|n| step_dist(v, &sol.choice, n))
        .collect();
    let fee = v.sc.fee_shift.clone();
    let elig = fee_eligibility(v, fee.as_ref())?;
    let mut nets = Vec::with_capacity(o.runs);
    let mut per_metric: Vec<Vec<f64>> = vec![Vec::with_capacity(o.runs); metrics.len()];
    let mut terms: BTreeMap<NodeIx, usize> = BTreeMap::new();
    let mut truncated = 0;
    let mut samples = vec![];
    let elapsed_ix = metrics.iter().position(|(k, _)| k == "elapsed");
    // `v` is only used through `dists`/`v.start` etc. inside the run loop;
    // rebind so each run starts fresh regardless of the caller's `start`.
    let v_from_start = View { start, ..v.clone() };
    for run in 0..o.runs {
        let keep_sample = run < o.keep_samples;
        let outcome = simulate_run(
            &v_from_start,
            &dists,
            fee.as_ref(),
            &elig,
            metrics,
            elapsed_ix,
            o,
            &mut rng,
            keep_sample,
        );
        if !outcome.ended {
            truncated += 1;
        }
        nets.push(outcome.net);
        if let Some(t) = outcome.terminal {
            *terms.entry(t).or_default() += 1;
        }
        for (i, a) in outcome.per_metric.into_iter().enumerate() {
            per_metric[i].push(a);
        }
        if let Some(traj) = outcome.traj {
            samples.push(traj);
        }
    }
    let mut sorted = nets.clone();
    sorted.sort_by(f64::total_cmp);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    // `alpha` is a fraction in [0, 1] and `sorted` is non-empty (`o.runs >= 1`),
    // so the ceil'd count is in `[0, sorted.len()]` before the clamp below.
    let k = ((o.alpha * sorted.len() as f64).ceil() as usize)
        .max(1)
        .min(sorted.len());
    let cvar = sorted[..k].iter().sum::<f64>() / k as f64;
    let p_loss = nets.iter().filter(|x| **x < 0.0).count() as f64 / nets.len() as f64;
    let mut terminals: Vec<(NodeIx, f64)> = terms
        .into_iter()
        .map(|(t, c)| (t, c as f64 / o.runs as f64))
        .collect();
    terminals.sort_by(|a, b| b.1.total_cmp(&a.1));
    Ok(SimResult {
        runs: o.runs,
        seed: o.seed,
        net: summarize(nets),
        cvar,
        alpha: o.alpha,
        p_loss,
        metrics: metrics
            .iter()
            .map(|(k, _)| k.clone())
            .zip(per_metric.into_iter().map(summarize))
            .collect(),
        terminals,
        truncated,
        samples,
    })
}
