// SPDX-License-Identifier: GPL-3.0-or-later
//! Scenario = everything that parametrizes an analysis without editing packs:
//! parameters, custom metrics, perspective, masks (waivers / counterfactuals),
//! probability and payoff overrides, and the modeling choices v1 buried in
//! code (mixed-node semantics, opponent model, probability fill).
//!
//! [`View::new`] resolves a scenario against a graph into dense per-edge /
//! per-node arrays that every algorithm consumes, and records every fallback
//! it had to take as a structured [`Warning`].

mod library;
mod limits;
mod plan;
mod resolve;
mod view;

pub use library::{NamedScenario, ScenarioSource};
pub use plan::fill;
pub use view::View;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::metrics::Builtin;
use crate::model::Role;

/// How to read a node where a chooser acts and world edges also leave.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum MixedMode {
    /// World edges fire with their authored probabilities; with the residual
    /// mass the chooser picks. Falls back to `act-or-wait` when the world
    /// edges carry no probabilities (or take all the mass).
    #[default]
    NatureFirst,
    /// The chooser picks one of its own edges, or waits and lets the world
    /// edges fire ("discovery proceeds unless you move to compel").
    ActOrWait,
    /// The chooser picks among its own edges; world edges are ignored (v1 chain semantics).
    SelfOnly,
    /// The chooser may take any out-edge, including the world's (v1 value-iteration semantics).
    Optimistic,
}

/// How to model a node where only the opponent chooses.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OpponentMode {
    /// Authored probabilities if complete, else adversarial.
    #[default]
    Auto,
    /// The opponent minimizes our value (zero-sum).
    Adversarial,
    /// The opponent's choices are draws (probability fill applies).
    Chance,
}

/// How to fill missing probabilities at a draw.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ProbFill {
    /// Authored probabilities kept; unauthored siblings split the residual equally.
    #[default]
    Residual,
    /// Any missing probability → uniform over all siblings (v1 behavior).
    Uniform,
}

/// What the solver optimizes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Objective {
    /// Maximize expected (utility − cost).
    #[default]
    Expected,
    /// Exponential utility with absolute risk aversion `a` (per dollar),
    /// solved exactly via certainty equivalents; `a > 0` is risk-averse.
    Cara {
        /// Absolute risk aversion per dollar.
        a: f64,
    },
    /// Every draw goes against us (robust / worst case).
    Worst,
    /// Maximize `CVaR_alpha` of the total outcome (the mean of the worst
    /// `alpha` fraction) — not just report it from a simulation. Solved by
    /// Rockafellar–Uryasev: one backward-induction pass over the graph
    /// computes, for every node and a grid of "remaining budget" values `y`,
    /// the optimal expected shortfall below `y`; the outer choice of `VaR`
    /// threshold `ζ` is then a lookup on that same grid at the start node.
    /// Ignores `discount_annual` and `fee_shift` (a warning is reported if
    /// either is set). See `docs/CRITIQUE.md` ("CVaR-optimal policies") for
    /// the method and its exactness limits (grid discretization; cyclic
    /// components are iterated to convergence, same as ordinary `solve`).
    Cvar {
        /// Tail fraction, e.g. `0.1` = worst 10%. Must be in `(0, 1]`.
        alpha: f64,
        /// Grid points spanning the outcome range, for both the outer `VaR`
        /// search and the inner augmented-state discretization (0 = default
        /// 41).
        #[serde(default)]
        grid: usize,
        /// Override the default outcome-range bound `(lo, hi)` the grid
        /// spans (default: the terminal-utility range widened by the
        /// graph's total absolute edge cost — a conservative bound that can
        /// be a poor fit for a graph with costly cycles, or unnecessarily
        /// wide otherwise; see `docs/CRITIQUE.md`). Set both or neither;
        /// `y_lo` must be strictly less than `y_hi`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        y_lo: Option<f64>,
        /// See `y_lo`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        y_hi: Option<f64>,
    },
}

/// Fee shifting: part of our cost is recovered if the matter ends at an eligible terminal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeeShift {
    /// Fraction of our accumulated cost recovered at an eligible terminal.
    pub fraction: f64,
    /// Terminal expression selecting eligible terminals (default `tag("fee-eligible")`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eligible: Option<String>,
}

/// A what-if: everything about one matter that is not in the packs.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, JsonSchema)]
#[serde(default, rename_all = "snake_case", deny_unknown_fields)]
pub struct Scenario {
    /// Parameter values visible to every expression (defaults: see `describe`).
    pub params: BTreeMap<String, f64>,
    /// Named custom edge metrics (name → expression); shadow built-ins.
    pub metrics: BTreeMap<String, String>,
    /// Named custom terminal utilities (name → expression).
    pub utilities: BTreeMap<String, String>,
    /// Metric used as the cost (name or expression). Default `dollars`.
    pub cost: Option<String>,
    /// Terminal utility (name or expression). Default `ev`.
    pub utility: Option<String>,
    /// actor → role override (e.g. analyze as the defendant).
    pub perspective: BTreeMap<String, Role>,
    /// Edge expression; edges where it is 0 are removed (counterfactual / waiver).
    pub mask: Option<String>,
    /// Edge refs to delete.
    pub remove_edges: Vec<String>,
    /// Edge ref → probability; authored siblings are rescaled to keep the node's mass.
    pub probabilities: BTreeMap<String, f64>,
    /// Edge expression rewriting `p` on every world edge (NaN = unauthored;
    /// non-finite results keep the original). Chance nodes are renormalized.
    pub probability_fn: Option<String>,
    /// Node ref → payoff override (USD).
    pub payoffs: BTreeMap<String, f64>,
    /// Node ref → edge ref: force our choice (a world edge at an act-or-wait node means "wait").
    pub policy: BTreeMap<String, String>,
    /// Node ref → edge ref: a matter fact this scenario knows to be true, at a
    /// chance node tagged `fact` in the pack (e.g. "is this claim time-barred").
    /// Forces that edge's probability to 1 (siblings to 0). Unlike `policy`,
    /// this applies at nodes nature controls, not at a choice of ours. A
    /// `fact`-tagged node with no entry here uses the pack's authored prior
    /// and a `fact-unset` warning names it. See `docs/PACK_SCHEMA.md#matter-facts`.
    pub facts: BTreeMap<String, String>,
    /// Mixed-node semantics.
    pub mixed: MixedMode,
    /// Opponent model where only the opponent chooses.
    pub opponent: OpponentMode,
    /// Missing-probability fill.
    pub prob_fill: ProbFill,
    /// Objective.
    pub objective: Objective,
    /// Annual discount rate applied along `elapsed` time (e.g. 0.08).
    pub discount_annual: Option<f64>,
    /// Fee shifting.
    pub fee_shift: Option<FeeShift>,
    /// Named calibration set(s) to apply, in order (see `calibration/*.json`;
    /// `litgraph schema calibration`). Each entry's probability/duration/node
    /// attr becomes "authored" exactly as if hand-typed into the pack — the
    /// usual probability-fill/renormalization and duration fallbacks still
    /// apply on top. A ref that doesn't resolve (its pack wasn't loaded) is
    /// silently skipped, like a `links.json` link into an unloaded pack.
    /// `provenance.calibration` on the response says what was actually
    /// applied, with each value's source, vintage, and sample size.
    pub calibration: Vec<String>,
    /// Opponent's own terminal payoff expression (e.g. their fees plus the
    /// stake). When set, the opponent is solved as a self-interested
    /// (general-sum) player via backward induction on the graph's SCC DAG —
    /// a subgame-perfect equilibrium — instead of a zero-sum adversary: at
    /// every opponent-controlled node the opponent picks the edge
    /// maximizing their own continuation value
    /// (`-opponent_dollars(edge) + opponent_objective(to)`), regardless of
    /// `opponent` mode. Cyclic components are iterated to a joint fixed
    /// point (both players' values), with the same convergence reporting as
    /// `solve`. `None` (default) keeps the existing zero-sum
    /// adversarial/chance opponent model, which the general-sum solver
    /// reduces to exactly. Not combined with `fee_shift` (a warning is
    /// reported if both are set: the opponent equilibrium uses unadjusted
    /// cost). See `docs/CRITIQUE.md` ("General-sum opponents").
    pub opponent_objective: Option<String>,
    /// Start node (default: the first pack's start).
    pub start: Option<String>,
}

/// Who acts at a node under the scenario.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Control {
    /// Absorbing.
    Terminal,
    /// Non-terminal with no active out-edges (a content bug or a mask effect); valued 0.
    Sink,
    /// A draw.
    Chance,
    /// We choose.
    Me,
    /// The opponent chooses.
    Opponent,
}

/// A fallback or modeling choice the engine had to make.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Warning {
    /// Stable machine code.
    pub code: &'static str,
    /// Qualified node id, if local.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    /// Explanation and how to fix it.
    pub message: String,
}

/// How a node's out-edges split between draws and a choice.
#[derive(Debug, Clone, PartialEq)]
pub struct NodePlan {
    /// Who acts.
    pub control: Control,
    /// Edges the chooser may pick.
    pub choices: Vec<usize>,
    /// `(edge, probability)` that fire before/instead of the choice (the whole
    /// distribution at chance nodes).
    pub draws: Vec<(usize, f64)>,
    /// Probability mass left for the choice (1 − Σ draws).
    pub choice_mass: f64,
    /// The chooser minimizes (adversarial opponent / worst-case nature).
    pub minimize: bool,
    /// Act-or-wait: the chooser may instead WAIT, letting these world edges fire.
    pub wait: Vec<(usize, f64)>,
}

impl NodePlan {
    pub(crate) fn of(control: Control) -> NodePlan {
        NodePlan {
            control,
            choices: vec![],
            draws: vec![],
            choice_mass: 0.0,
            minimize: false,
            wait: vec![],
        }
    }
}

/// Sentinel "edge" recorded in a solution's choice map when the chooser waits.
pub const WAIT: usize = usize::MAX;

/// Resolve a metric/utility spec: a scenario-defined name, a built-in name,
/// or else the spec itself as an inline expression.
#[must_use]
pub fn resolve_spec<'a>(
    spec: &'a str,
    custom: &'a BTreeMap<String, String>,
    builtins: &'a [Builtin],
) -> &'a str {
    custom
        .get(spec)
        .map(String::as_str)
        .or_else(|| builtins.iter().find(|b| b.name == spec).map(|b| b.expr))
        .unwrap_or(spec)
}
