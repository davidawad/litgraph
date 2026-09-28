//! litgraph — litigation procedure as a stochastic game.
//!
//! Layers, bottom to top (each only depends on the ones below):
//!
//! 1. `model`    — packs (JSON) → one compiled, namespaced [`model::Graph`].
//! 2. `expr`     — the custom-function language (costs, utilities, masks).
//! 3. `metrics`  — built-in metrics/utilities/params *as expressions* + the
//!                 variables every expression can see.
//! 4. `scenario` — a what-if (params, perspective, masks, overrides, modeling
//!                 modes) resolved against a graph into a [`scenario::View`].
//! 5. `algo`     — solve (stochastic game), chain (exact expectations),
//!                 sim (distributions), paths/pareto, sweep/tornado, structure.
//! 6. `api`      — one JSON request → one JSON response with warnings and
//!                 provenance. The CLI and any future MCP server are thin
//!                 wrappers over this.

pub mod algo;
pub mod api;
pub mod error;
pub mod expr;
pub mod lint;
pub mod metrics;
pub mod model;
pub mod scenario;

pub use error::{Error, Result};
pub use model::{Graph, Pack};
pub use scenario::{Scenario, View};
