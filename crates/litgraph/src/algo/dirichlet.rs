// SPDX-License-Identifier: GPL-3.0-or-later
//! Seeded sampling from the Dirichlet family (Gamma, Dirichlet, multinomial)
//! and the empirical-quantile helper the uncertainty ops share.
//!
//! `rand` 0.8 ships uniform sampling only (the Gamma/normal distributions
//! live in `rand_distr`), so the two samplers the Dirichlet needs are
//! implemented here rather than adding a dependency:
//!
//! - standard normal by the Box–Muller transform;
//! - `Gamma(shape, 1)` by Marsaglia & Tsang (2000), "A simple method for
//!   generating gamma variables", *ACM Trans. Math. Softw.* 26(3):363–372,
//!   with the `shape < 1` boost `Gamma(a) = Gamma(a + 1) · U^(1/a)` from
//!   the same paper.
//!
//! A Dirichlet draw normalizes independent Gamma draws; a component whose
//! parameter is 0 is structurally impossible and always draws 0.

use rand::Rng;
use rand_chacha::ChaCha8Rng;

fn std_normal(rng: &mut ChaCha8Rng) -> f64 {
    // 1 − U ∈ (0, 1], so the log is finite.
    let u1: f64 = 1.0 - rng.gen::<f64>();
    let u2: f64 = rng.gen();
    (-2.0 * u1.ln()).sqrt() * (std::f64::consts::TAU * u2).cos()
}

/// One `Gamma(shape, 1)` draw (`shape > 0`; `0` for `shape <= 0`).
pub fn gamma(rng: &mut ChaCha8Rng, shape: f64) -> f64 {
    if shape <= 0.0 {
        return 0.0;
    }
    if shape < 1.0 {
        let u: f64 = 1.0 - rng.gen::<f64>();
        return gamma(rng, shape + 1.0) * u.powf(1.0 / shape);
    }
    let d = shape - 1.0 / 3.0;
    let c = 1.0 / (9.0 * d).sqrt();
    loop {
        let x = std_normal(rng);
        let v = (1.0 + c * x).powi(3);
        if v <= 0.0 {
            continue;
        }
        let u: f64 = 1.0 - rng.gen::<f64>();
        if u.ln() < 0.5 * x * x + d - d * v + d * v.ln() {
            return d * v;
        }
    }
}

/// One `Dirichlet(alpha)` draw. Zero components stay zero; an all-zero
/// `alpha` returns all zeros.
pub fn dirichlet(rng: &mut ChaCha8Rng, alpha: &[f64]) -> Vec<f64> {
    let mut x: Vec<f64> = alpha.iter().map(|&a| gamma(rng, a)).collect();
    let s: f64 = x.iter().sum();
    if s > 0.0 {
        for xi in &mut x {
            *xi /= s;
        }
    } else if let Some(i) = alpha.iter().position(|&a| a > 0.0) {
        // Every Gamma draw underflowed (tiny shapes): the limit of a
        // Dirichlet with vanishing concentration is a point mass on one
        // component; take the first positive one.
        x[i] = 1.0;
    }
    x
}

/// Counts of `k` independent categorical draws from `p` (a multinomial draw).
pub fn multinomial(rng: &mut ChaCha8Rng, k: u64, p: &[f64]) -> Vec<f64> {
    let mut counts = vec![0.0; p.len()];
    let total: f64 = p.iter().sum();
    if p.is_empty() || total <= 0.0 {
        return counts;
    }
    // Rounding can leave `u` just past the last bucket: land it on the last
    // possible (positive) outcome, never on an impossible one.
    let last = p.iter().rposition(|&x| x > 0.0).unwrap_or(0);
    for _ in 0..k {
        let mut u = rng.gen::<f64>() * total;
        let mut pick = last;
        for (i, &pi) in p.iter().enumerate() {
            if u < pi {
                pick = i;
                break;
            }
            u -= pi;
        }
        counts[pick] += 1.0;
    }
    counts
}

/// The `q`-quantile (`0..=1`) of `xs` by linear interpolation between
/// order statistics (sorts `xs`). `NaN` for an empty slice.
pub fn quantile(xs: &mut [f64], q: f64) -> f64 {
    if xs.is_empty() {
        return f64::NAN;
    }
    xs.sort_by(f64::total_cmp);
    let h = q.clamp(0.0, 1.0) * (xs.len() - 1) as f64;
    // `h` is in [0, len − 1], so the floor is a valid index.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let i = h.floor() as usize;
    let j = (i + 1).min(xs.len() - 1);
    xs[i] + (h - i as f64) * (xs[j] - xs[i])
}

/// Mean and standard error of the mean of `xs` (`(NaN, NaN)` if empty; the
/// error is `0` for a single sample).
#[must_use]
pub fn mean_se(xs: &[f64]) -> (f64, f64) {
    if xs.is_empty() {
        return (f64::NAN, f64::NAN);
    }
    let n = xs.len() as f64;
    let m = xs.iter().sum::<f64>() / n;
    if xs.len() < 2 {
        return (m, 0.0);
    }
    let var = xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (n - 1.0);
    (m, (var / n).sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    #[test]
    fn gamma_moments_match_shape() {
        let mut rng = ChaCha8Rng::seed_from_u64(1);
        for shape in [0.3, 1.0, 4.5] {
            let xs: Vec<f64> = (0..20_000).map(|_| gamma(&mut rng, shape)).collect();
            let (m, _) = mean_se(&xs);
            let var = xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / xs.len() as f64;
            assert!(
                (m - shape).abs() < 0.05 * shape.max(1.0),
                "{shape}: mean {m}"
            );
            assert!(
                (var - shape).abs() < 0.1 * shape.max(1.0),
                "{shape}: var {var}"
            );
        }
        assert_eq!(gamma(&mut rng, 0.0), 0.0);
    }

    #[test]
    fn dirichlet_means_and_zero_components() {
        let mut rng = ChaCha8Rng::seed_from_u64(2);
        let alpha = [2.0, 0.0, 6.0];
        let mut acc = [0.0; 3];
        for _ in 0..10_000 {
            let x = dirichlet(&mut rng, &alpha);
            assert!((x.iter().sum::<f64>() - 1.0).abs() < 1e-12);
            assert_eq!(x[1], 0.0);
            for (a, xi) in acc.iter_mut().zip(&x) {
                *a += xi / 10_000.0;
            }
        }
        assert!((acc[0] - 0.25).abs() < 0.01 && (acc[2] - 0.75).abs() < 0.01);
        assert_eq!(dirichlet(&mut rng, &[0.0, 0.0]), vec![0.0, 0.0]);
        // Vanishing shapes underflow every Gamma draw: point mass on the first positive.
        let tiny = dirichlet(&mut rng, &[0.0, 1e-300, 1e-300]);
        assert!((tiny.iter().sum::<f64>() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn multinomial_counts_sum_to_k() {
        let mut rng = ChaCha8Rng::seed_from_u64(3);
        let c = multinomial(&mut rng, 50, &[0.2, 0.0, 0.8]);
        assert_eq!(c.iter().sum::<f64>(), 50.0);
        assert_eq!(c[1], 0.0);
        assert_eq!(multinomial(&mut rng, 5, &[]), Vec::<f64>::new());
        assert_eq!(multinomial(&mut rng, 5, &[0.0, 0.0]), vec![0.0, 0.0]);
    }

    #[test]
    fn quantiles_and_standard_errors() {
        let mut xs = vec![3.0, 1.0, 2.0, 4.0];
        assert_eq!(quantile(&mut xs, 0.0), 1.0);
        assert_eq!(quantile(&mut xs, 1.0), 4.0);
        assert!((quantile(&mut xs, 0.5) - 2.5).abs() < 1e-12);
        assert!(quantile(&mut [], 0.5).is_nan());
        assert_eq!(mean_se(&[5.0]), (5.0, 0.0));
        assert!(mean_se(&[]).0.is_nan());
        let (m, se) = mean_se(&[1.0, 3.0]);
        assert_eq!(m, 2.0);
        assert!((se - 1.0).abs() < 1e-12);
    }
}
