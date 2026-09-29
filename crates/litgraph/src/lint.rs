// SPDX-License-Identifier: GPL-3.0-or-later
//! Content QA. Structural errors fail compilation; everything here is a
//! diagnostic an author (human or agent) can act on.

use serde::Serialize;
use std::collections::{HashMap, HashSet};

use crate::cite::{CiteOutcome, SourceCorpus};
use crate::model::{Graph, NodeKind, Pack, RawEdge};

/// One content-quality finding: a warning about authored data, not a
/// structural error (those fail compilation instead).
#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    /// `"error"`, `"warn"`, or `"info"`, in descending severity.
    pub severity: &'static str,
    /// Stable diagnostic code (e.g. `"terminal-no-payoff"`), for branching.
    pub code: &'static str,
    /// The pack/node/edge id the diagnostic is about.
    pub at: String,
    /// Human-readable explanation.
    pub message: String,
}

/// A `Diagnostic` sink used while walking a pack or graph.
type Push<'a> = dyn FnMut(&'static str, &'static str, String, String) + 'a;

/// Runs content QA over the compiled graph and its source packs.
///
/// This never panics: every lookup it performs (pack starts, node ids) is
/// against data the graph itself already validated at compile time, and any
/// lookup that still fails is reported as a diagnostic rather than unwrapped.
#[must_use]
pub fn lint(g: &Graph, packs: &[Pack]) -> Vec<Diagnostic> {
    let mut d = vec![];
    {
        let mut push = |severity, code, at: String, message: String| {
            d.push(Diagnostic {
                severity,
                code,
                at,
                message,
            });
        };
        let corpus = SourceCorpus::default_source();
        for p in packs {
            lint_pack_sources(p, &mut push);
            lint_pack_edges(p, &mut push);
            lint_pack_nodes(p, &mut push);
            match &corpus {
                Ok(corpus) if corpus.is_empty() => push(
                    "info",
                    "sources-unavailable",
                    p.id.clone(),
                    "this build embeds no L0 source texts (feature `embed-sources` off); \
                     set LITGRAPH_SOURCES to a sources/ directory to verify cites"
                        .to_string(),
                ),
                Ok(corpus) => lint_pack_cites(p, corpus, &mut push),
                Err(e) => push(
                    "info",
                    "sources-unavailable",
                    p.id.clone(),
                    format!("could not load the L0 source corpus, cite verification skipped: {e}"),
                ),
            }
        }
    }
    lint_reachability(g, &mut d);
    d.sort_by_key(|x| match x.severity {
        "error" => 0,
        "warn" => 1,
        _ => 2,
    });
    d
}

fn lint_pack_sources(p: &Pack, push: &mut Push<'_>) {
    let pid = &p.id;
    if p.schema_version < 2 {
        push(
            "info",
            "schema-v1",
            pid.clone(),
            "v1 pack: no roles/payoffs/durations/sources; the engine falls back to heuristics"
                .into(),
        );
    }
    for s in &p.sources {
        let local = s
            .path
            .as_deref()
            .is_some_and(|x| x.starts_with('/') || x.starts_with('~') || x.contains(":\\"));
        if local {
            push(
                "error",
                "local-source-path",
                format!("{pid}::sources::{}", s.id),
                "source `path` must be repo-relative; cite the official `url` instead of a local file".into(),
            );
        }
    }
    if p.sources.is_empty() {
        push(
            "warn",
            "no-sources",
            pid.clone(),
            "no `sources`; cites cannot be traced to a primary document".into(),
        );
    }
}

/// Every `cite`/`authority` should resolve to a span in a vendored L0
/// source this pack itself declares (`sources[].path`). A pack with no
/// `sources` at all is already flagged by [`lint_pack_sources`]'s
/// `no-sources`, so this only reports per-cite once the pack has at least
/// one source to check against (otherwise every cite would duplicate the
/// same "no sources" reason).
fn lint_pack_cites(p: &Pack, corpus: &SourceCorpus, push: &mut Push<'_>) {
    if p.sources.is_empty() {
        return;
    }
    let pid = &p.id;
    for check in crate::cite::check_pack(p, corpus) {
        if let CiteOutcome::Unresolvable { reason } = check.outcome {
            push(
                "warn",
                "unverifiable-cite",
                format!("{pid}::{}", check.at),
                format!("cite `{}` in `{}` {reason}", check.cite_ref.raw, check.raw),
            );
        }
    }
}

fn lint_pack_edges(p: &Pack, push: &mut Push<'_>) {
    let pid = &p.id;
    let mut par: HashMap<(&str, &str), usize> = HashMap::new();
    for e in &p.edges {
        *par.entry((&e.from, &e.to)).or_default() += 1;
    }
    for e in &p.edges {
        let at = format!("{pid}::{}->{}", e.from, e.to);
        if e.actor == "applicant" && e.probability.is_some() {
            push(
                "error",
                "probability-on-choice",
                at.clone(),
                format!("`{}` is a choice but carries a probability", e.label),
            );
        }
        if par[&(e.from.as_str(), e.to.as_str())] > 1 && e.id.is_none() {
            push(
                "warn",
                "parallel-edge-no-id",
                at.clone(),
                format!(
                    "parallel edge `{}` has no stable id (v1 keyed Q-values by from→to and silently collided here)",
                    e.label
                ),
            );
        }
        if e.authority.is_none() && e.actor == "applicant" {
            push(
                "info",
                "no-authority",
                at.clone(),
                format!("choice `{}` has no authority cite", e.label),
            );
        }
        if let Some(dl) = &e.deadline {
            if dl.length <= 0.0 {
                push(
                    "error",
                    "bad-deadline",
                    at.clone(),
                    "deadline length must be > 0".into(),
                );
            }
        }
        if let Some(du) = &e.duration {
            if du.min.is_some_and(|m| m > du.mode) || du.max.is_some_and(|m| m < du.mode) {
                push(
                    "error",
                    "bad-duration",
                    at.clone(),
                    "duration must satisfy min ≤ mode ≤ max".into(),
                );
            }
        }
    }
}

fn lint_pack_nodes(p: &Pack, push: &mut Push<'_>) {
    let pid = &p.id;
    for n in &p.nodes {
        let at = format!("{pid}::{}", n.id);
        let outs: Vec<_> = p.edges.iter().filter(|e| e.from == n.id).collect();
        let terminal = n.kind == Some(NodeKind::Terminal);
        if terminal && n.payoff.is_none() {
            push(
                "warn",
                "terminal-no-payoff",
                at.clone(),
                "terminal without `payoff`; valued by heuristic/0".into(),
            );
        }
        if terminal && n.outcome.is_empty() && p.schema_version >= 2 {
            push(
                "info",
                "terminal-no-outcome",
                at.clone(),
                "terminal without `outcome` tags".into(),
            );
        }
        if !terminal && outs.is_empty() {
            push(
                "error",
                "dead-end",
                at.clone(),
                "non-terminal with no out-edges".into(),
            );
        }
        let is_fact = n.tags.iter().any(|t| t == "fact");
        if !terminal && !outs.is_empty() && outs.iter().all(|e| e.actor != "applicant") {
            let with = outs.iter().filter(|e| e.probability.is_some()).count();
            if with == outs.len() {
                let s: f64 = outs.iter().filter_map(|e| e.probability).sum();
                if (s - 1.0).abs() > 1e-3 {
                    push(
                        "error",
                        "probability-sum",
                        at.clone(),
                        format!("chance node probabilities sum to {s:.3}"),
                    );
                }
            } else if is_fact {
                // A node tagged `fact` is a matter fact knowable at filing
                // (e.g. "is this claim time-barred"), not real chance — an
                // unauthored prior means it silently defaults to a uniform
                // (or residual) split instead of a real base rate whenever
                // `scenario.facts` isn't set for this matter.
                push(
                    "warn",
                    "fact-no-prior",
                    at.clone(),
                    format!(
                        "tagged `fact` but only {with}/{} out-edges carry an authored prior probability; author base-rate estimates (basis + vintage in `note`) so an unset scenario.facts falls back to something better than an even split",
                        outs.len()
                    ),
                );
            } else if outs.len() > 1 {
                push(
                    "info",
                    "chance-unquantified",
                    at.clone(),
                    format!("{}/{} outcome probabilities authored", with, outs.len()),
                );
            }
        }
        let mixed = outs.iter().any(|e| e.actor == "applicant")
            && outs.iter().any(|e| e.actor != "applicant");
        if mixed {
            push(
                "info",
                "mixed-node",
                at.clone(),
                "choices and world edges share this node; see scenario.mixed".into(),
            );
            if is_fact {
                lint_mixed_fact_node(&outs, &at, push);
            }
        }
    }
}

/// The pure-chance-node `fact-no-prior` check above is gated on "every
/// out-edge is non-applicant", so it never ran for a *mixed* node (a choice
/// edge plus fact-driven interrupts, e.g. ptab's petition-threshold-review:
/// refile vs. the office's time-bar determination). Check the same thing on
/// just the interrupt (non-applicant) edges -- scenario/plan.rs's `chooser()`
/// reads the same `authored` array `chance()` does.
fn lint_mixed_fact_node(outs: &[&RawEdge], at: &str, push: &mut Push<'_>) {
    let interrupts: Vec<&RawEdge> = outs
        .iter()
        .copied()
        .filter(|e| e.actor != "applicant")
        .collect();
    let with = interrupts
        .iter()
        .filter(|e| e.probability.is_some())
        .count();
    if with < interrupts.len() {
        push(
            "warn",
            "fact-no-prior",
            at.to_string(),
            format!(
                "tagged `fact` but only {with}/{} interrupt out-edges carry an authored prior probability; author base-rate estimates (basis + vintage in `note`) so an unset scenario.facts falls back to something better than an even split",
                interrupts.len()
            ),
        );
    }
}

/// Reachability from each pack start. Instances (`base@origin`) are entered
/// through links, not their start, so they are skipped.
///
/// Checked at the `base_id` level, not the exact compiled node: a state-flag
/// product graph gives every base node its own empty-flag copy (so any node
/// stays directly addressable as `scenario.start`), but a node that's only
/// ever reached with a flag set (e.g. downstream of an IPR estoppel or a
/// waived defense) has no *real* predecessor into its unflagged copy. That's
/// expected, not a content defect — so a base node only warns when *none* of
/// its compiled copies (flagged or not) are reachable.
fn lint_reachability(g: &Graph, d: &mut Vec<Diagnostic>) {
    for p in g.packs.iter().filter(|p| !p.id.contains('@')) {
        let s = match g.node(&p.start) {
            Ok(s) => s,
            Err(err) => {
                d.push(Diagnostic {
                    severity: "error",
                    code: "bad-pack-start",
                    at: p.id.clone(),
                    message: format!("pack start `{}` does not resolve to a node: {err}", p.start),
                });
                continue;
            }
        };
        let mut seen = vec![false; g.nodes.len()];
        let mut stack = vec![s];
        seen[s] = true;
        while let Some(u) = stack.pop() {
            for &e in &g.out[u] {
                let w = g.edges[e].to;
                if !seen[w] {
                    seen[w] = true;
                    stack.push(w);
                }
            }
        }
        let reachable_base: HashSet<&str> = (0..g.nodes.len())
            .filter(|&i| seen[i])
            .map(|i| g.nodes[i].base_id.as_str())
            .collect();
        let mut warned: HashSet<&str> = HashSet::new();
        for n in &g.nodes {
            if n.pack == p.id
                && !n.synthetic
                && !reachable_base.contains(n.base_id.as_str())
                && warned.insert(n.base_id.as_str())
            {
                d.push(Diagnostic {
                    severity: "warn",
                    code: "unreachable",
                    at: n.base_id.clone(),
                    message: format!("not reachable from {} start", p.id),
                });
            }
        }
    }
}

#[cfg(test)]
#[path = "lint_tests.rs"]
mod tests;
