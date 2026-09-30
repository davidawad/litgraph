// SPDX-License-Identifier: GPL-3.0-or-later
//! Posterior propagation: Monte Carlo over every chance node's Dirichlet
//! posterior (`scenario::Belief`), re-solving the game per draw.
//!
//! Each draw samples an independent `θ_n ~ Dirichlet(α_n)` at every uncertain
//! node, writes it into a scratch copy of the view, and runs the ordinary
//! risk-neutral `mdp::solve`. Across draws that gives a credible interval on
//! the case value and on each option's Q at a decision node, and the
//! posterior probability that each option is the optimal one — the
//! "probability of being optimal" a decision maker actually wants when the
//! probabilities are soft. `nominal` is the same quantity at the posterior
//! mean (what `solve` reports).

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use serde::Serialize;

use super::dirichlet;
use super::mdp::{self, SolveOptions};
use crate::error::{Error, Result};
use crate::model::NodeIx;
use crate::scenario::{Objective, View, WAIT};

/// Monte Carlo settings.
#[derive(Debug, Clone)]
pub struct PosteriorOptions {
    /// Posterior draws (each one a full solve).
    pub samples: usize,
    /// RNG seed.
    pub seed: u64,
    /// Central credible-interval mass, in `(0, 1)`.
    pub credibility: f64,
}

/// A posterior summary of one quantity.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Interval {
    /// Value at the posterior mean (what `solve` reports).
    pub nominal: f64,
    /// Posterior mean over draws.
    pub mean: f64,
    /// Lower end of the central credible interval.
    pub lo: f64,
    /// Upper end.
    pub hi: f64,
}

impl Interval {
    fn of(nominal: f64, xs: &mut [f64], credibility: f64) -> Interval {
        let (mean, _) = dirichlet::mean_se(xs);
        Interval {
            nominal,
            mean,
            lo: dirichlet::quantile(xs, (1.0 - credibility) / 2.0),
            hi: dirichlet::quantile(xs, f64::midpoint(1.0, credibility)),
        }
    }
}

/// One option at the decision node.
#[derive(Debug, Clone)]
pub struct OptionStat {
    /// The edge (or [`WAIT`]).
    pub edge: usize,
    /// Its Q across draws.
    pub q: Interval,
    /// Fraction of draws in which the mover picks it.
    pub p_optimal: f64,
}

/// The propagated posterior.
#[derive(Debug, Clone)]
pub struct Posterior {
    /// Value at the start node.
    pub value: Interval,
    /// The decision node's options (empty if nobody chooses there).
    pub options: Vec<OptionStat>,
    /// The option chosen at the posterior mean.
    pub nominal_choice: Option<usize>,
    /// Uncertain nodes sampled per draw.
    pub uncertain: Vec<usize>,
    /// Draws whose solve hit an iteration cap.
    pub unconverged: usize,
}

/// A copy of `v` for re-solving under hypothetical probabilities: the
/// uncertainty ops value decisions risk-neutrally (see `docs/UNCERTAINTY.md`).
#[must_use]
pub fn risk_neutral<'g>(v: &View<'g>) -> View<'g> {
    let mut s = v.clone();
    s.sc.objective = Objective::Expected;
    s
}

/// The options a mover has at `n`: its choices, plus `WAIT` at an act-or-wait node.
#[must_use]
pub fn options_at(v: &View, n: NodeIx) -> Vec<usize> {
    let p = &v.plan[n];
    let mut out = p.choices.clone();
    if !p.wait.is_empty() {
        out.push(WAIT);
    }
    out
}

/// Propagate the posterior to the value at `start` and the options at `node`.
///
/// # Errors
/// Zero `samples`, `credibility` outside `(0, 1)`, or a solve error.
pub fn posterior(v: &View, start: NodeIx, node: NodeIx, o: &PosteriorOptions) -> Result<Posterior> {
    if o.samples == 0 {
        return Err(Error::Invalid(
            "posterior: samples must be at least 1".into(),
        ));
    }
    if !(o.credibility > 0.0 && o.credibility < 1.0) {
        return Err(Error::Invalid(format!(
            "posterior: credibility must be in (0, 1), got {}",
            o.credibility
        )));
    }
    let opts = SolveOptions::default();
    let mut s = risk_neutral(v);
    let base = mdp::solve(&s, &opts)?;
    let options = options_at(&s, node);
    let uncertain = s.belief.uncertain();
    let alphas: Vec<Vec<f64>> = uncertain
        .iter()
        .map(|&g| s.belief.groups[g].alpha())
        .collect();
    let mut rng = ChaCha8Rng::seed_from_u64(o.seed);
    let mut values = Vec::with_capacity(o.samples);
    let mut qs = vec![Vec::with_capacity(o.samples); options.len()];
    let mut wins = vec![0usize; options.len()];
    let mut unconverged = 0;
    for _ in 0..o.samples {
        for (k, &gi) in uncertain.iter().enumerate() {
            let x = dirichlet::dirichlet(&mut rng, &alphas[k]);
            s.set_group(gi, &x);
        }
        let sol = mdp::solve(&s, &opts)?;
        unconverged += usize::from(!sol.converged);
        values.push(sol.value[start]);
        for (j, &e) in options.iter().enumerate() {
            qs[j].push(sol.option_q(&s, node, e));
        }
        if let Some(j) = sol
            .choice
            .get(&node)
            .and_then(|c| options.iter().position(|e| e == c))
        {
            wins[j] += 1;
        }
    }
    let n = o.samples as f64;
    let options = options
        .iter()
        .zip(qs.iter_mut())
        .zip(&wins)
        .map(|((&e, xs), &w)| OptionStat {
            edge: e,
            q: Interval::of(base.option_q(v, node, e), xs, o.credibility),
            p_optimal: w as f64 / n,
        })
        .collect();
    Ok(Posterior {
        value: Interval::of(base.value[start], &mut values, o.credibility),
        options,
        nominal_choice: base.choice.get(&node).copied(),
        uncertain,
        unconverged,
    })
}
