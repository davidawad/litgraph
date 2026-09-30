// SPDX-License-Identifier: GPL-3.0-or-later
//! The typed operation vocabulary. Unknown ops or arguments are rejected
//! with the list of valid ones, so a typo never silently runs the wrong thing.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::algo::settle::{Rule68, Side, Sides};
use crate::clock::ServiceMethod;
use crate::scenario::Objective;

#[allow(clippy::wildcard_imports)] // serde `default = "..."` paths name these helpers
use super::op_defaults::*;

/// What `structure` computes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StructureWhat {
    /// Counts.
    #[default]
    Summary,
    /// Cycles (strongly connected components).
    Scc,
    /// Immediate dominators, or the unavoidable gateways to `to`.
    Dominators,
    /// Min cut to `to` under `capacity`.
    Mincut,
    /// Betweenness centrality.
    Betweenness,
    /// Reachable terminals / unreachable nodes.
    Reachability,
}

/// A study to price in `voi`: evidence worth `k` observations of a chance
/// node's outcome (e.g. an expert report, a mock-trial panel, a survey of
/// this judge's rulings), compared against what it costs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StudySpec {
    /// The chance node the study observes.
    pub node: String,
    /// How many observations it is worth.
    pub k: u64,
    /// Its price in the scenario's cost units (USD by default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    /// Or: the edge that buys it; its price is that edge's scenario cost metric.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_edge: Option<String>,
    /// Display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// `settle`'s arguments (see [`Op::Settle`]).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SettleArgs {
    /// Node to evaluate from (default: start).
    #[serde(default)]
    pub node: Option<String>,
    /// Which side `self` is on (prices are what the defendant pays).
    #[serde(default)]
    pub self_side: Side,
    /// The opponent's risk attitude (`self`'s is `scenario.objective`):
    /// `{type: expected}` | `{type: cara, a}` | `{type: worst}` |
    /// `{type: cvar, alpha}` (`CVaR` by Monte Carlo).
    #[serde(default)]
    pub opponent_risk: Objective,
    /// The plaintiff's Nash bargaining power in `[0, 1]`.
    #[serde(default = "f_half")]
    pub bargaining_power: f64,
    /// Per-side annual discount rates driving Rubinstein patience
    /// (default: `scenario.discount_annual`).
    #[serde(default)]
    pub discount_annual: Sides,
    /// Per-side per-round discount factors in `(0, 1]`, overriding the rates.
    #[serde(default)]
    pub delta: Sides,
    /// A Rule 68 offer of judgment served by the defendant at `node`.
    #[serde(default)]
    pub rule68: Option<Rule68>,
    /// Monte Carlo runs per node for a `CVaR` side.
    #[serde(default = "n::<20000>")]
    pub runs: usize,
    /// RNG seed for those runs.
    #[serde(default = "seed")]
    pub seed: u64,
    /// Timing-line length cap.
    #[serde(default = "n::<40>")]
    pub max_steps: usize,
}

/// One operation. `from` overrides the scenario's start node.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Op {
    /// The machine-readable manual.
    #[default]
    Describe,
    /// Packs with data-quality statistics.
    Packs,
    /// Content diagnostics for the selected packs.
    Lint,
    /// Resolve the scenario against the packs without running anything.
    Validate,
    /// Nodes and edges (a node's neighborhood if `node`).
    Graph {
        /// Neighborhood center.
        #[serde(default)]
        node: Option<String>,
    },
    /// Evaluate a metric on every edge (test custom functions here).
    Metric {
        /// Metric name or expression.
        spec: String,
        /// Rows to return.
        #[serde(default = "n::<25>")]
        top: usize,
    },
    /// Who decides at a node, and every option with value, regret, cost and what follows.
    Explain {
        /// Node (default: start).
        #[serde(default)]
        node: Option<String>,
        /// Start override.
        #[serde(default)]
        from: Option<String>,
    },
    /// Value and best line under the scenario.
    Solve {
        /// Start override.
        #[serde(default)]
        from: Option<String>,
        /// Include every chosen option.
        #[serde(default)]
        full_policy: bool,
        /// Include every node's value.
        #[serde(default)]
        all_values: bool,
        /// Best-line length cap.
        #[serde(default = "n::<40>")]
        max_steps: usize,
    },
    /// Exact absorption probabilities and expected totals under the optimal policy.
    Chain {
        /// Start override.
        #[serde(default)]
        from: Option<String>,
        /// Metrics to total (first one is netted against utility).
        #[serde(default = "chain_metrics", deserialize_with = "one_or_many")]
        metrics: Vec<String>,
        /// Most-visited nodes to list.
        #[serde(default = "n::<15>")]
        top: usize,
    },
    /// Monte Carlo outcome distribution, `CVaR` and P(loss).
    Simulate {
        /// Start override.
        #[serde(default)]
        from: Option<String>,
        /// Number of runs.
        #[serde(default = "n::<10000>")]
        runs: usize,
        /// RNG seed.
        #[serde(default = "seed")]
        seed: u64,
        /// Metrics to summarize.
        #[serde(default = "sim_metrics", deserialize_with = "one_or_many")]
        metrics: Vec<String>,
        /// `CVaR` tail fraction.
        #[serde(default = "f_alpha")]
        alpha: f64,
        /// Step cap per run.
        #[serde(default = "n::<10000>")]
        max_steps: usize,
        /// Sample durations from (min, mode, max).
        #[serde(default = "yes")]
        sample_durations: bool,
        /// Sample trajectories to return.
        #[serde(default = "n::<3>")]
        samples: usize,
    },
    /// Shortest or k-shortest lines to `to` (node, `terminals`, or `tag:<outcome>`).
    Path {
        /// Start override.
        #[serde(default)]
        from: Option<String>,
        /// Target spec.
        #[serde(default = "terminals")]
        to: String,
        /// Metric to minimize.
        #[serde(default = "dollars")]
        metric: String,
        /// Number of alternatives.
        #[serde(default = "n::<1>")]
        k: usize,
        /// Extra metrics to total along each path.
        #[serde(default, deserialize_with = "one_or_many")]
        report: Vec<String>,
    },
    /// N-objective Pareto frontier to `to`.
    Pareto {
        /// Start override.
        #[serde(default)]
        from: Option<String>,
        /// Target spec.
        #[serde(default = "terminals")]
        to: String,
        /// Objectives (metric names or expressions, all non-negative).
        #[serde(default = "pareto_objectives", deserialize_with = "one_or_many")]
        objectives: Vec<String>,
        /// Label budget.
        #[serde(default = "n::<200000>")]
        max_labels: usize,
        /// Frontier paths to return.
        #[serde(default = "n::<25>")]
        limit: usize,
    },
    /// Value curve and policy breakpoints as any parameter moves.
    Sweep {
        /// Parameter name (any, including ones only a custom function reads).
        param: String,
        /// Low end.
        #[serde(default = "f_100")]
        lo: f64,
        /// High end.
        #[serde(default = "f_1500")]
        hi: f64,
        /// Grid points.
        #[serde(default = "n::<15>")]
        steps: usize,
        /// Nodes to watch (default all).
        #[serde(default, deserialize_with = "one_or_many")]
        watch: Vec<String>,
        /// Breakpoint tolerance.
        #[serde(default = "f_tol")]
        tol: f64,
    },
    /// Sensitivity of the value to parameters and authored probabilities.
    Tornado {
        /// Parameters to perturb.
        #[serde(default = "tornado_params", deserialize_with = "one_or_many")]
        params: Vec<String>,
        /// Relative perturbation for parameters.
        #[serde(default = "f_rel")]
        rel: f64,
        /// Absolute perturbation for probabilities.
        #[serde(default = "f_dp")]
        dp: f64,
        /// Include authored probabilities.
        #[serde(default = "yes")]
        probabilities: bool,
        /// Rows to return.
        #[serde(default = "n::<20>")]
        top: usize,
    },
    /// Uncalibrated probabilities and durations, ranked by decision
    /// sensitivity — what to calibrate first.
    Calibration {
        /// Rows to return, per kind (probabilities / durations).
        #[serde(default = "n::<20>")]
        top: usize,
        /// Absolute perturbation for uncalibrated probabilities.
        #[serde(default = "f_dp")]
        dp: f64,
        /// Relative perturbation for uncalibrated durations.
        #[serde(default = "f_rel")]
        rel: f64,
        /// Cap on edges scanned per kind (bounds cost on a large selection).
        #[serde(default = "n::<300>")]
        max_candidates: usize,
    },
    /// Monte Carlo over the Dirichlet posterior of every chance node: a
    /// credible interval on the case value and on each option's Q at a
    /// decision node, and the probability each option is optimal.
    Posterior {
        /// Start override.
        #[serde(default)]
        from: Option<String>,
        /// Decision node whose options to report (default: start).
        #[serde(default)]
        node: Option<String>,
        /// Posterior draws (each a full solve).
        #[serde(default = "n::<200>")]
        samples: usize,
        /// RNG seed.
        #[serde(default = "seed")]
        seed: u64,
        /// Central credible-interval mass.
        #[serde(default = "f_credibility")]
        credibility: f64,
    },
    /// Value of information: EVPI of each chance node's outcome and
    /// probabilities, EVPI of everything at once, and EVSI of priced
    /// studies — ranked, and compared against each study's cost.
    Voi {
        /// Start override.
        #[serde(default)]
        from: Option<String>,
        /// Draws per Monte Carlo estimate.
        #[serde(default = "n::<200>")]
        samples: usize,
        /// RNG seed.
        #[serde(default = "seed")]
        seed: u64,
        /// Rows (ranked by outcome EVPI) that also get the Monte Carlo
        /// parameter EVPPI.
        #[serde(default = "n::<10>")]
        top: usize,
        /// Cap on uncertain nodes screened (bounds cost on a large selection).
        #[serde(default = "n::<200>")]
        max_nodes: usize,
        /// Studies to price (EVSI vs cost).
        #[serde(default)]
        studies: Vec<StudySpec>,
    },
    /// Structural analyses.
    Structure {
        /// Start override.
        #[serde(default)]
        from: Option<String>,
        /// Which analysis.
        #[serde(default)]
        what: StructureWhat,
        /// Target node (dominators, mincut).
        #[serde(default)]
        to: Option<String>,
        /// Edge capacity metric for mincut.
        #[serde(default = "one")]
        capacity: String,
        /// Rows to return.
        #[serde(default = "n::<15>")]
        top: usize,
    },
    /// Concrete due dates, with computation steps, for authored pack
    /// `deadline` specs (`FRCP 6` / `RCFC 6` / `FRAP 26` / `19 CFR
    /// 210.6(a)`, chosen from the owning pack's `forum`/id).
    Deadlines {
        /// The triggering event date, `"YYYY-MM-DD"`.
        trigger: String,
        /// Compute only this edge's deadline (overrides `node`/`reachable`).
        #[serde(default)]
        edge: Option<String>,
        /// Node whose out-edges to compute (default: start).
        #[serde(default)]
        node: Option<String>,
        /// Walk every edge reachable from `node` (not just its immediate
        /// out-edges).
        #[serde(default)]
        reachable: bool,
        /// Service method adding days before the last-day roll
        /// (6(d)/26(c)/201.16), if any.
        #[serde(default)]
        service_method: Option<ServiceMethod>,
        /// State-declared or presidential/congressional holidays beyond
        /// the computed federal set, each `"YYYY-MM-DD"`.
        #[serde(default)]
        additional_holidays: Vec<String>,
        /// Clerk's office inaccessible on the last day (`FRCP`/`RCFC
        /// 6(a)(3)`; ignored with a step note under rule sets that don't
        /// define it).
        #[serde(default)]
        clerk_inaccessible: bool,
    },
    /// Settlement prediction from both sides' values: the bargaining range
    /// (ZOPA) or the no-deal gap, Nash / Rubinstein / midpoint prices, when
    /// the surplus peaks along the likely line (settling as an
    /// always-available action), and an optional Rule 68 offer of judgment.
    /// Set `scenario.opponent_objective`; without it the opponent is zero-sum
    /// and there is never a surplus to split. See `docs/SETTLEMENT.md`.
    Settle(SettleArgs),
    /// Run `inner` under the scenario and under the scenario merge-patched with `variant`.
    Compare {
        /// JSON merge-patch applied to the scenario.
        #[serde(default)]
        variant: serde_json::Value,
        /// The op to compare (default `chain`).
        #[serde(default = "chain_op")]
        inner: Box<Op>,
    },
    /// Several ops against one request's packs and scenario.
    Batch {
        /// Ops.
        ops: Vec<Op>,
    },
}

impl Op {
    /// The op's wire name.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Op::Describe => "describe",
            Op::Packs => "packs",
            Op::Lint => "lint",
            Op::Validate => "validate",
            Op::Graph { .. } => "graph",
            Op::Metric { .. } => "metric",
            Op::Explain { .. } => "explain",
            Op::Solve { .. } => "solve",
            Op::Chain { .. } => "chain",
            Op::Simulate { .. } => "simulate",
            Op::Path { .. } => "path",
            Op::Pareto { .. } => "pareto",
            Op::Sweep { .. } => "sweep",
            Op::Tornado { .. } => "tornado",
            Op::Calibration { .. } => "calibration",
            Op::Posterior { .. } => "posterior",
            Op::Voi { .. } => "voi",
            Op::Structure { .. } => "structure",
            Op::Deadlines { .. } => "deadlines",
            Op::Settle(_) => "settle",
            Op::Compare { .. } => "compare",
            Op::Batch { .. } => "batch",
        }
    }
}

#[cfg(test)]
#[path = "op_tests.rs"]
mod tests;
