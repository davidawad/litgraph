// SPDX-License-Identifier: GPL-3.0-or-later
//! Unit and property tests for `bargain.rs`.

use proptest::prelude::*;

use super::*;

fn z(p: f64, d: f64) -> Zopa {
    Zopa {
        plaintiff: p,
        defendant: d,
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1.0)
}

#[test]
fn midpoint_and_linear_nash_are_hand_computable() {
    let r = z(5000.0, 7000.0);
    assert!(r.deal());
    assert_eq!(r.surplus(), 2000.0);
    assert_eq!(r.midpoint(), 6000.0);
    assert_eq!(nash_price(&r, 0.5, 0.0, 0.0), 6000.0);
    assert_eq!(nash_price(&r, 0.25, 0.0, 0.0), 5500.0);
    assert_eq!(nash_price(&r, 0.0, 1e-3, 1e-3), 5000.0);
    assert_eq!(nash_price(&r, 1.0, 1e-3, 1e-3), 7000.0);
}

#[test]
fn symmetric_cara_nash_is_the_midpoint_and_risk_aversion_concedes() {
    let r = z(0.0, 100.0);
    assert!(close(nash_price(&r, 0.5, 0.02, 0.02), 50.0));
    // A risk-averse plaintiff against a risk-neutral defendant settles lower.
    assert!(nash_price(&r, 0.5, 0.05, 0.0) < 50.0);
    // ... and a risk-averse defendant pays more.
    assert!(nash_price(&r, 0.5, 0.0, 0.05) > 50.0);
}

#[test]
fn no_deal_prices_collapse_to_the_plaintiff_reservation() {
    let r = z(10.0, 10.0);
    assert!(!r.deal());
    assert_eq!(nash_price(&r, 0.3, 0.1, 0.0), 10.0);
    assert!(!z(10.0, 5.0).deal());
}

#[test]
fn rubinstein_matches_the_closed_form() {
    let r = z(5000.0, 7000.0);
    let pat = Patience {
        delta_p: 1.0 / 1.1,
        delta_d: 1.0 / 1.2,
        rate_p: Some(0.1),
        rate_d: Some(0.2),
    };
    let rb = rubinstein(&r, &pat);
    // Plaintiff proposes: keeps (1 - δd)/(1 - δp·δd) = 0.6875 of 2000.
    assert!(close(rb.plaintiff_first, 6375.0));
    // Defendant proposes: plaintiff gets δp·(1 - δd)/(1 - δp·δd) = 0.625.
    assert!(close(rb.defendant_first, 6250.0));
    let share = 1.2f64.ln() / (1.1f64.ln() + 1.2f64.ln());
    assert!(close(rb.limit_share, share));
    assert!(close(rb.limit, 5000.0 + 2000.0 * share));
}

#[test]
fn rubinstein_with_no_elapsed_time_uses_the_rates_limit_or_splits_evenly() {
    let r = z(0.0, 100.0);
    let timeless = |rate_p, rate_d| Patience {
        delta_p: 1.0,
        delta_d: 1.0,
        rate_p,
        rate_d,
    };
    let rb = rubinstein(&r, &timeless(Some(0.1), Some(0.1)));
    assert!(close(rb.plaintiff_first, 50.0) && close(rb.defendant_first, 50.0));
    let rb = rubinstein(&r, &timeless(None, None));
    assert!(close(rb.limit, 50.0));
    let rb = rubinstein(&r, &timeless(Some(0.0), Some(0.3)));
    assert!(
        close(rb.limit, 100.0),
        "the perfectly patient side takes it all"
    );
}

#[test]
fn round_delta_discounts_by_elapsed_days() {
    assert!(close(round_delta(0.1, 365.0), 1.0 / 1.1));
    assert_eq!(round_delta(0.1, 0.0), 1.0);
}

proptest! {
    /// Every model's price lies inside the ZOPA.
    #[test]
    fn prices_are_inside_the_zopa(
        lo in -1e6f64..1e6, width in 1e-3f64..1e6, beta in 0.0f64..=1.0,
        a_p in 0.0f64..1e-3, a_d in 0.0f64..1e-3,
        dp in 0.01f64..=1.0, dd in 0.01f64..=1.0,
    ) {
        let r = z(lo, lo + width);
        let inside = |p: f64| p >= r.plaintiff - 1e-6 && p <= r.defendant + 1e-6;
        prop_assert!(inside(r.midpoint()));
        prop_assert!(inside(nash_price(&r, beta, a_p, a_d)));
        let rb = rubinstein(&r, &Patience { delta_p: dp, delta_d: dd, rate_p: None, rate_d: None });
        prop_assert!(inside(rb.plaintiff_first));
        prop_assert!(inside(rb.defendant_first));
        prop_assert!(inside(rb.limit));
        // Moving first is never worse for the proposer.
        prop_assert!(rb.plaintiff_first >= rb.defendant_first - 1e-6);
    }

    /// A range with zero (or negative) surplus is never a deal.
    #[test]
    fn zero_or_negative_surplus_is_no_deal(x in -1e9f64..1e9, gap in 0.0f64..1e6) {
        prop_assert!(!z(x, x).deal());
        prop_assert!(!z(x + gap, x).deal());
    }
}
