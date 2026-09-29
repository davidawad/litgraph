// SPDX-License-Identifier: GPL-3.0-or-later
//! Kani proofs (`cargo kani -p litgraph`). Bounded, fast, and about
//! properties the rest of the engine relies on.

use crate::api::catalog_fnv1a64;
use crate::expr::truthy;
use crate::scenario::{fill, ProbFill};

/// Distinct single bytes never collide, so a one-byte edit to a pack always
/// changes its fingerprint; hashing is deterministic.
#[kani::proof]
fn fnv1a64_is_injective_on_single_bytes() {
    let a: u8 = kani::any();
    let b: u8 = kani::any();
    kani::assume(a != b);
    assert_ne!(catalog_fnv1a64(&[a]), catalog_fnv1a64(&[b]));
    assert_eq!(catalog_fnv1a64(&[a, b]), catalog_fnv1a64(&[a, b]));
}

/// The expression language's truthiness: zero and NaN are false, everything else true.
#[kani::proof]
fn truthiness_law() {
    let x: f64 = kani::any();
    assert_eq!(truthy(x), !(x == 0.0 || x.is_nan()));
}

/// A representative probability, or "unauthored" (`None`). A handful of
/// discrete values (0, a quarter, a half, all of it) exercise every branch
/// `fill` takes (complete/missing, over/under 1) without CBMC having to
/// bit-blast an unconstrained symbolic `f64` — that combinatorial explosion,
/// not the property itself, is what made an earlier version of this harness
/// take minutes instead of seconds.
const PROBS: [f64; 4] = [0.0, 0.25, 0.5, 1.0];

fn probability(pick: u8) -> Option<f64> {
    if pick == 0 {
        None
    } else {
        Some(PROBS[(pick as usize - 1) % PROBS.len()])
    }
}

/// Filling a two-way draw always yields finite, non-negative weights that
/// sum to 1 — whatever mix of authored/missing probabilities, in either mode.
#[kani::proof]
#[kani::unwind(4)]
fn fill_is_a_distribution() {
    let (pa, pb): (u8, u8) = (kani::any(), kani::any());
    kani::assume(pa <= PROBS.len() as u8 && pb <= PROBS.len() as u8);
    let authored = [probability(pa), probability(pb)];
    let mode = if kani::any() {
        ProbFill::Residual
    } else {
        ProbFill::Uniform
    };
    let dist = fill(&[0, 1], &authored, mode);
    assert_eq!(dist.len(), 2);
    let mut sum = 0.0;
    for &(_, w) in &dist {
        assert!(w.is_finite() && w >= 0.0);
        sum += w;
    }
    assert!((sum - 1.0).abs() < 1e-9);
}
