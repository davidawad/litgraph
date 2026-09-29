// SPDX-License-Identifier: GPL-3.0-or-later
//! Calibration: real, sourced probabilities/durations/node attrs applied as
//! a named overlay on the compiled graph, instead of hand-typed "teaching
//! estimate" numbers in a pack.
//!
//! A calibration set is data, not code: `calibration/*.json` files, embedded
//! at build time exactly like `packs/*.json` and overridable at runtime via
//! `$LITGRAPH_CALIBRATION` (or the CLI's `--calibration-dir`). Every entry
//! records the value (or, for a duration, a distribution), a source URL and
//! title, the vintage (reporting period) it covers, and the sample size `n`
//! behind it ([`model`]). [`apply::apply`] mutates a compiled graph the same
//! way an authored pack value would: probability-fill, renormalization, and
//! duration fallbacks all apply exactly as if the number had been hand-typed
//! into the pack, because that is exactly what calibration is — a better
//! source for a number the pack would otherwise guess at or leave
//! unauthored. [`gaps::gaps`] ranks what calibration hasn't reached yet.
//!
//! See `docs/CALIBRATION.md` for the file format and worked examples.

mod apply;
mod gaps;
mod model;

pub use apply::{apply, apply_all, apply_named, Application, Applied};
pub use gaps::{gaps, Gap, Gaps};
pub use model::{
    CalibrationCatalog, CalibrationEntry, CalibrationSet, CalibrationSource, CalibrationTarget,
};
