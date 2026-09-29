// SPDX-License-Identifier: GPL-3.0-or-later
//! Request timing that works on every target, including wasm32-unknown-unknown.

/// Wall-clock timer. `std::time::Instant` panics on `wasm32-unknown-unknown`
/// (no clock), so there the timer reports 0 instead of aborting the request.
pub(super) struct Stopwatch(
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))] std::time::Instant,
);

impl Stopwatch {
    pub(super) fn start() -> Stopwatch {
        Stopwatch(
            #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
            std::time::Instant::now(),
        )
    }

    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    pub(super) fn elapsed_ms(&self) -> f64 {
        (self.0.elapsed().as_secs_f64() * 1e6).round() / 1e3
    }

    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    #[allow(clippy::unused_self)] // no clock on this target
    pub(super) fn elapsed_ms(&self) -> f64 {
        0.0
    }
}
