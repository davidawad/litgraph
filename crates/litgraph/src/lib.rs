// SPDX-License-Identifier: GPL-3.0-or-later
//! litgraph — litigation procedure as a stochastic game.
//!
//! Layers, bottom to top (each only depends on the ones below):
//!
//! - `model`: packs (JSON) → one compiled, namespaced [`model::Graph`].
//! - `expr`: the custom-function language (costs, utilities, masks).
//! - `metrics`: built-in metrics/utilities/params *as expressions* + the
//!   variables every expression can see.
//! - `scenario`: a what-if (params, perspective, masks, overrides, modeling
//!   modes) resolved against a graph into a [`scenario::View`].
//! - `algo`: solve (stochastic game), chain (exact expectations),
//!   sim (distributions), paths/pareto, sweep/tornado, structure.
//! - `api`: one JSON request → one JSON response with warnings and
//!   provenance. The CLI and any future MCP server are thin
//!   wrappers over this.

/// Algorithms over a resolved [`scenario::View`]: solve, chain, simulate,
/// paths/pareto, sweep/tornado, structure (SCCs, dominators, min-cut).
pub mod algo;
pub mod api;
/// Cite verification against the vendored L0 corpus (`sources/*.txt`).
pub mod cite;
/// Deadline computation: `FRCP 6`/`RCFC 6`/`FRAP 26`/`19 CFR 210.6(a)`
/// court-day and calendar-day time computation.
pub mod clock;
/// The crate's error type and `Result` alias.
pub mod error;
pub mod expr;
pub mod lint;
pub mod metrics;
pub mod model;
pub mod scenario;

#[cfg(kani)]
mod proofs;

pub use error::{Error, Result};
pub use model::{Graph, Pack};
pub use scenario::{Scenario, View};
