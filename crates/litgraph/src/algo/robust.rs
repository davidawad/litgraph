// SPDX-License-Identifier: GPL-3.0-or-later
//! Robust solve: maximize the worst-case expected value over a credible set
//! of probabilities (`objective: {type: robust}`).
//!
//! **Method.** A rectangular robust MDP (Iyengar 2005, "Robust dynamic
//! programming", *Math. Oper. Res.* 30(2):257–280; Nilim & El Ghaoui 2005,
//! "Robust control of Markov decision processes with uncertain transition
//! matrices", *Oper. Res.* 53(5):780–798): each chance draw's distribution
//! is chosen by nature, independently per node ("rectangular"), from an
//! ambiguity set `P_n`, and the Bellman backup takes the worst case
//!
//! ```text
//! V(n) = min_{p ∈ P_n} Σ_i p_i · q_i
//! ```
//!
//! inside the ordinary SCC-ordered solve (`mdp::solve`). Rectangularity is
//! what keeps this a dynamic program with a deterministic optimal policy.
//!
//! **Ambiguity set.** `P_n = {p ∈ Δ(S_n) : ‖p − θ̄_n‖₁ ≤ ψ_n}`, the L1 ball
//! around the posterior mean restricted to the posterior's support `S_n`
//! (outcomes with `α > 0`). The radius `ψ_n` is the `credibility` quantile of
//! `‖θ − θ̄_n‖₁` for `θ ~ Dirichlet(α_n)`, estimated from seeded posterior
//! draws — the Bayesian credible ambiguity set ("BCI") of Petrik & Russel
//! 2019, "Beyond confidence regions: tight Bayesian ambiguity sets for
//! robust MDPs", *NeurIPS* 32. A fixed `radius` replaces it everywhere.
//!
//! **Inner problem.** The L1-ball minimization has a greedy closed form:
//! move `ψ/2` of mass onto the lowest-value outcome, taking it from the
//! highest-value outcomes first. The optimistic mirror image is MBIE's
//! (Strehl & Littman 2008, *Journal of Computer and System Sciences*
//! 74(8):1309–1331); the robust form is used by e.g. Petrik & Subramanian
//! 2014, "RAAM", *NeurIPS* 27.
//!
//! **Limits.** Credibility is per node: the joint probability that *every*
//! node's true distribution lies in its set is lower (a union bound), so the
//! robust value is conservative at the node level, not a joint credible
//! bound. Opponent choices stay as modeled (`opponent` mode); only nature's
//! draws are ambiguous.

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

use super::dirichlet;
use crate::error::{Error, Result};
use crate::scenario::View;

/// Default posterior draws per node for the credible radius.
pub const DEFAULT_SAMPLES: usize = 2000;

/// `min Σ p_i q_i` over `p` in the simplex on the support of `terms`
/// (`(p̄_i, q_i)` with `p̄_i > 0`) with `‖p − p̄‖₁ ≤ radius`.
#[must_use]
pub fn worst_l1(terms: &[(f64, f64)], radius: f64) -> f64 {
    let mut idx: Vec<usize> = (0..terms.len()).filter(|&i| terms[i].0 > 0.0).collect();
    if idx.is_empty() {
        return 0.0;
    }
    idx.sort_by(|&a, &b| terms[a].1.total_cmp(&terms[b].1));
    let mut p: Vec<f64> = terms.iter().map(|t| t.0).collect();
    let lo = idx[0];
    let add = (radius / 2.0).min(1.0 - p[lo]).max(0.0);
    p[lo] += add;
    let mut take = add;
    for &i in idx.iter().rev() {
        if take <= 0.0 || i == lo {
            break;
        }
        let d = take.min(p[i]);
        p[i] -= d;
        take -= d;
    }
    idx.iter().map(|&i| p[i] * terms[i].1).sum()
}

/// The ambiguity radius at every node (`0` where nothing is uncertain).
///
/// # Errors
/// `credibility` outside `(0, 1)` or a negative / non-finite `radius`.
pub fn radii(
    v: &View,
    credibility: f64,
    radius: Option<f64>,
    samples: usize,
    seed: u64,
) -> Result<Vec<f64>> {
    if !(credibility > 0.0 && credibility < 1.0) {
        return Err(Error::Invalid(format!(
            "objective robust: credibility must be in (0, 1), got {credibility}"
        )));
    }
    if let Some(r) = radius {
        if !r.is_finite() || r < 0.0 {
            return Err(Error::Invalid(format!(
                "objective robust: radius must be a non-negative number, got {r}"
            )));
        }
    }
    let samples = if samples == 0 {
        DEFAULT_SAMPLES
    } else {
        samples
    };
    let mut out = vec![0.0; v.g.nodes.len()];
    for gi in v.belief.uncertain() {
        let grp = &v.belief.groups[gi];
        out[grp.node] = if let Some(r) = radius {
            r
        } else {
            let alpha = grp.alpha();
            let mean = grp.mean();
            let mut rng = ChaCha8Rng::seed_from_u64(seed ^ (grp.node as u64));
            let mut d: Vec<f64> = (0..samples)
                .map(|_| {
                    dirichlet::dirichlet(&mut rng, &alpha)
                        .iter()
                        .zip(&mean)
                        .map(|(x, m)| (x - m).abs())
                        .sum()
                })
                .collect();
            dirichlet::quantile(&mut d, credibility)
        };
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worst_l1_moves_half_the_radius_to_the_worst_outcome() {
        let terms = [(0.5, 10.0), (0.3, 0.0), (0.2, 5.0)];
        // Nominal 6.0; radius 0.4 moves 0.2 from the 10-outcome to the 0-outcome.
        assert!((worst_l1(&terms, 0.0) - 6.0).abs() < 1e-12);
        assert!((worst_l1(&terms, 0.4) - 4.0).abs() < 1e-12);
        // Radius 2 reaches the worst supported outcome.
        assert!(worst_l1(&terms, 2.0).abs() < 1e-12);
        // Outcomes outside the support are never used, whatever their value.
        assert!((worst_l1(&[(1.0, 3.0), (0.0, -100.0)], 2.0) - 3.0).abs() < 1e-12);
        assert_eq!(worst_l1(&[(0.0, 1.0)], 1.0), 0.0);
        // Mass comes from the best outcome first, then the next.
        let w = worst_l1(&[(0.1, 10.0), (0.4, 8.0), (0.5, 0.0)], 0.6);
        assert!((w - 0.2 * 8.0).abs() < 1e-12, "{w}");
    }
}
