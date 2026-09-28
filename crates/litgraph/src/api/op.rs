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
        OneOrMany::One(s) => s
            .split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect(),
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
    Box::new(Op::Chain {
        from: None,
        metrics: chain_metrics(),
        top: 15,
    })
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

    /// Borrows `self` as the narrower vocabulary `run_view` actually
    /// dispatches on, or `None` for `Describe`/`Lint`/`Packs`/`Batch`/
    /// `Compare` — the five ops `handle` always answers itself before a
    /// [`crate::scenario::View`] would even exist. Exists so `run_view`'s
    /// match can be exhaustive over exactly the ops it can receive, with no
    /// "can't happen here" arm left for a wildcard to paper over.
    #[must_use]
    pub(super) fn as_view_op(&self) -> Option<ViewOp<'_>> {
        Some(match self {
            Op::Describe | Op::Lint | Op::Packs | Op::Batch { .. } | Op::Compare { .. } => {
                return None
            }
            Op::Validate => ViewOp::Validate,
            Op::Graph { node } => ViewOp::Graph { node },
            Op::Metric { spec, top } => ViewOp::Metric { spec, top: *top },
            Op::Explain { node, from } => ViewOp::Explain { node, from },
            Op::Solve {
                from,
                full_policy,
                all_values,
                max_steps,
            } => ViewOp::Solve {
                from,
                full_policy: *full_policy,
                all_values: *all_values,
                max_steps: *max_steps,
            },
            Op::Chain { from, metrics, top } => ViewOp::Chain {
                from,
                metrics,
                top: *top,
            },
            Op::Simulate {
                from,
                runs,
                seed,
                metrics,
                alpha,
                max_steps,
                sample_durations,
                samples,
            } => ViewOp::Simulate {
                from,
                runs: *runs,
                seed: *seed,
                metrics,
                alpha: *alpha,
                max_steps: *max_steps,
                sample_durations: *sample_durations,
                samples: *samples,
            },
            Op::Path {
                from,
                to,
                metric,
                k,
                report,
            } => ViewOp::Path {
                from,
                to,
                metric,
                k: *k,
                report,
            },
            Op::Pareto {
                from,
                to,
                objectives,
                max_labels,
                limit,
            } => ViewOp::Pareto {
                from,
                to,
                objectives,
                max_labels: *max_labels,
                limit: *limit,
            },
            Op::Sweep {
                param,
                lo,
                hi,
                steps,
                watch,
                tol,
            } => ViewOp::Sweep {
                param,
                lo: *lo,
                hi: *hi,
                steps: *steps,
                watch,
                tol: *tol,
            },
            Op::Tornado {
                params,
                rel,
                dp,
                probabilities,
                top,
            } => ViewOp::Tornado {
                params,
                rel: *rel,
                dp: *dp,
                probabilities: *probabilities,
                top: *top,
            },
            Op::Structure {
                from,
                what,
                to,
                capacity,
                top,
            } => ViewOp::Structure {
                from,
                what: *what,
                to,
                capacity,
                top: *top,
            },
        })
    }
}

/// The subset of [`Op`] that needs a resolved [`crate::scenario::View`] —
/// borrowed out of an `&Op` by [`Op::as_view_op`]. `run_view` matches this
/// exhaustively, so adding an `Op` variant that needs a view is a compile
/// error here until it's threaded through, and one that doesn't need a view
/// never has to be considered by `run_view` at all.
#[derive(Debug, Clone, Copy)]
pub(super) enum ViewOp<'a> {
    Validate,
    Graph {
        node: &'a Option<String>,
    },
    Metric {
        spec: &'a str,
        top: usize,
    },
    Explain {
        node: &'a Option<String>,
        from: &'a Option<String>,
    },
    Solve {
        from: &'a Option<String>,
        full_policy: bool,
        all_values: bool,
        max_steps: usize,
    },
    Chain {
        from: &'a Option<String>,
        metrics: &'a [String],
        top: usize,
    },
    Simulate {
        from: &'a Option<String>,
        runs: usize,
        seed: u64,
        metrics: &'a [String],
        alpha: f64,
        max_steps: usize,
        sample_durations: bool,
        samples: usize,
    },
    Path {
        from: &'a Option<String>,
        to: &'a str,
        metric: &'a str,
        k: usize,
        report: &'a [String],
    },
    Pareto {
        from: &'a Option<String>,
        to: &'a str,
        objectives: &'a [String],
        max_labels: usize,
        limit: usize,
    },
    Sweep {
        param: &'a str,
        lo: f64,
        hi: f64,
        steps: usize,
        watch: &'a [String],
        tol: f64,
    },
    Tornado {
        params: &'a [String],
        rel: f64,
        dp: f64,
        probabilities: bool,
        top: usize,
    },
    Structure {
        from: &'a Option<String>,
        what: StructureWhat,
        to: &'a Option<String>,
        capacity: &'a str,
        top: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A single comma-separated string is split on `,`, trimmed, and empty
    /// segments are dropped.
    #[test]
    fn one_or_many_splits_and_trims_a_comma_string() -> serde_json::Result<()> {
        let op: Op = serde_json::from_value(serde_json::json!({
            "op": "chain",
            "metrics": "dollars, elapsed,, hours "
        }))?;
        match op {
            Op::Chain { metrics, .. } => {
                assert_eq!(metrics, vec!["dollars", "elapsed", "hours"]);
            }
            other => panic!("expected Chain, got {other:?}"),
        }
        Ok(())
    }

    /// A JSON array is taken as-is (the `Many` branch of `OneOrMany`).
    #[test]
    fn one_or_many_accepts_an_array() -> serde_json::Result<()> {
        let op: Op = serde_json::from_value(serde_json::json!({
            "op": "chain",
            "metrics": ["dollars", "hours"]
        }))?;
        match op {
            Op::Chain { metrics, .. } => assert_eq!(metrics, vec!["dollars", "hours"]),
            other => panic!("expected Chain, got {other:?}"),
        }
        Ok(())
    }
}
