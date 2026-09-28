// SPDX-License-Identifier: GPL-3.0-or-later
//! The typed operation vocabulary. Unknown ops or arguments are rejected
//! with the list of valid ones, so a typo never silently runs the wrong thing.

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

fn one_or_many<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    Ok(match OneOrMany::deserialize(d)? {
        OneOrMany::One(s) => s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect(),
        OneOrMany::Many(v) => v,
    })
}

fn v(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| (*s).to_string()).collect()
}
fn chain_metrics() -> Vec<String> {
    v(&["dollars", "elapsed", "hours"])
}
fn sim_metrics() -> Vec<String> {
    v(&["dollars", "elapsed"])
}
fn pareto_objectives() -> Vec<String> {
    v(&["dollars", "elapsed", "surprise"])
}
fn tornado_params() -> Vec<String> {
    v(&["rate", "stakes"])
}
fn terminals() -> String {
    "terminals".into()
}
fn dollars() -> String {
    "dollars".into()
}
fn one() -> String {
    "1".into()
}
fn yes() -> bool {
    true
}
const fn n<const N: usize>() -> usize {
    N
}
fn f_100() -> f64 {
    100.0
}
fn f_1500() -> f64 {
    1500.0
}
fn f_tol() -> f64 {
    1e-3
}
fn f_alpha() -> f64 {
    0.1
}
fn f_rel() -> f64 {
    0.25
}
fn f_dp() -> f64 {
    0.1
}
fn seed() -> u64 {
    7
}
// serde's `default = "chain_op"` requires this to return exactly the
// field's type (`Box<Op>`); returning `Op` would not satisfy that bound.
#[allow(clippy::unnecessary_box_returns)]
fn chain_op() -> Box<Op> {
    Box::new(Op::Chain { from: None, metrics: chain_metrics(), top: 15 })
}

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

/// One operation. `from` overrides the scenario's start node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Op {
    /// The machine-readable manual.
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
    /// Monte Carlo outcome distribution, CVaR and P(loss).
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
        /// CVaR tail fraction.
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

impl Default for Op {
    fn default() -> Self {
        Op::Describe
    }
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
            Op::Structure { .. } => "structure",
            Op::Compare { .. } => "compare",
            Op::Batch { .. } => "batch",
        }
    }
}
