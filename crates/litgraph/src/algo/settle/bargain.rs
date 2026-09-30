// SPDX-License-Identifier: GPL-3.0-or-later
//! Bargaining models over a settlement range: pure functions of the two
//! reservation prices, no graph.
//!
//! Prices are what the defendant pays the plaintiff. The plaintiff accepts
//! any price at or above its reservation `r_p`; the defendant pays any price
//! at or below its reservation `r_d`. The range `[r_p, r_d]` is the zone of
//! possible agreement (ZOPA) and `r_d - r_p` the settlement surplus.
//!
//! - **Split the surplus** (Gould 1973's midpoint): `r_p + S/2`.
//! - **Nash bargaining** (Nash 1950; the weighted form is Kalai 1977): the
//!   price maximizing `(u_p(P) - u_p(r_p))^β · (u_d(-P) - u_d(-r_d))^(1-β)`.
//!   With linear (risk-neutral) money utility it is `r_p + β·S`; with CARA
//!   utility the more risk-averse side concedes more.
//! - **Rubinstein alternating offers** (Rubinstein 1982): with per-round
//!   discount factors `δ_p`, `δ_d`, the first proposer keeps
//!   `(1 - δ_other)/(1 - δ_p·δ_d)` of the surplus. As the round length
//!   shrinks, both orders converge to the Nash split with power
//!   `ρ_d/(ρ_p + ρ_d)` (Binmore, Rubinstein & Wolinsky 1986).
//!
//! See `docs/SETTLEMENT.md` for the full citations.

use serde::Serialize;

/// A settlement range: the two reservation prices and what lies between.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Zopa {
    /// The lowest price the plaintiff accepts (its walk-away point).
    pub plaintiff: f64,
    /// The highest price the defendant pays (its walk-away point).
    pub defendant: f64,
}

impl Zopa {
    /// `defendant - plaintiff`: positive inside a deal zone, otherwise the
    /// negated gap.
    #[must_use]
    pub fn surplus(&self) -> f64 {
        self.defendant - self.plaintiff
    }

    /// True if the surplus is strictly positive (beyond rounding). A zero
    /// surplus leaves nothing to gain from settling, so it is no deal.
    #[must_use]
    pub fn deal(&self) -> bool {
        let tol = 1e-9 * self.plaintiff.abs().max(self.defendant.abs()).max(1.0);
        self.surplus() > tol
    }

    /// The price giving the plaintiff `share` of the surplus.
    #[must_use]
    pub fn at_share(&self, share: f64) -> f64 {
        self.plaintiff + share.clamp(0.0, 1.0) * self.surplus()
    }

    /// The split-the-surplus midpoint.
    #[must_use]
    pub fn midpoint(&self) -> f64 {
        self.at_share(0.5)
    }
}

/// `u'(x) / (u(x) - u(0))` for a side's money utility at gain `x > 0`:
/// `1/x` when risk-neutral (`a == 0`), `a / (e^(a·x) - 1)` under CARA.
fn marginal_ratio(a: f64, x: f64) -> f64 {
    if a == 0.0 {
        1.0 / x
    } else {
        a / (a * x).exp_m1()
    }
}

/// The (weighted) Nash bargaining price: maximizes the Nash product of each
/// side's utility gain over its reservation, with plaintiff bargaining power
/// `beta` in `[0, 1]` and CARA coefficients `a_p`/`a_d` (`0` = risk-neutral).
/// Returns the plaintiff's reservation when there is no deal.
#[must_use]
pub fn nash_price(z: &Zopa, beta: f64, a_p: f64, a_d: f64) -> f64 {
    let beta = beta.clamp(0.0, 1.0);
    if !z.deal() {
        return z.plaintiff;
    }
    if a_p == 0.0 && a_d == 0.0 {
        return z.at_share(beta);
    }
    if beta == 0.0 || beta == 1.0 {
        return z.at_share(beta);
    }
    // The log Nash product is strictly concave on the open range; its
    // derivative is decreasing from +inf to -inf, so bisect on its sign.
    let s = z.surplus();
    let (mut lo, mut hi) = (0.0, s);
    for _ in 0..200 {
        let x = 0.5 * (lo + hi);
        let slope = beta * marginal_ratio(a_p, x) - (1.0 - beta) * marginal_ratio(a_d, s - x);
        if slope > 0.0 {
            lo = x;
        } else {
            hi = x;
        }
    }
    z.plaintiff + 0.5 * (lo + hi)
}

/// Per-round discount factor for an annual rate over `days`.
#[must_use]
pub fn round_delta(rate: f64, days: f64) -> f64 {
    (1.0 + rate).powf(-days / 365.0)
}

/// Rubinstein alternating-offers prices over a range.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Rubinstein {
    /// Plaintiff's per-round discount factor.
    pub delta_plaintiff: f64,
    /// Defendant's per-round discount factor.
    pub delta_defendant: f64,
    /// Price when the plaintiff makes the first offer.
    pub plaintiff_first: f64,
    /// Price when the defendant makes the first offer.
    pub defendant_first: f64,
    /// Price as the round length goes to zero (either order).
    pub limit: f64,
    /// The plaintiff's share of the surplus in that limit.
    pub limit_share: f64,
}

/// Patience inputs for [`rubinstein`]: per-round discount factors, plus the
/// annual rates they came from (used for the limit when both are 1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Patience {
    /// Plaintiff's per-round discount factor, in `(0, 1]`.
    pub delta_p: f64,
    /// Defendant's per-round discount factor, in `(0, 1]`.
    pub delta_d: f64,
    /// Plaintiff's annual rate, if known.
    pub rate_p: Option<f64>,
    /// Defendant's annual rate, if known.
    pub rate_d: Option<f64>,
}

/// The Rubinstein (1982) subgame-perfect prices over `z`.
#[must_use]
pub fn rubinstein(z: &Zopa, pat: &Patience) -> Rubinstein {
    let (dp, dd) = (pat.delta_p, pat.delta_d);
    let den = 1.0 - dp * dd;
    // Impatience per round, `ρ = -ln δ`; when both factors are 1 (no time
    // passes, or no rate given) fall back on the annual rates themselves,
    // and on an even split when neither side is impatient at all.
    let (mut rp, mut rd) = (-dp.ln(), -dd.ln());
    if rp + rd <= 0.0 {
        rp = pat.rate_p.map_or(0.0, f64::ln_1p);
        rd = pat.rate_d.map_or(0.0, f64::ln_1p);
    }
    let limit_share = if rp + rd > 0.0 { rd / (rp + rd) } else { 0.5 };
    let (pf, df) = if den > 1e-12 {
        ((1.0 - dd) / den, dp * (1.0 - dd) / den)
    } else {
        (limit_share, limit_share)
    };
    Rubinstein {
        delta_plaintiff: dp,
        delta_defendant: dd,
        plaintiff_first: z.at_share(pf),
        defendant_first: z.at_share(df),
        limit: z.at_share(limit_share),
        limit_share,
    }
}

#[cfg(test)]
#[path = "bargain_tests.rs"]
mod tests;
