// SPDX-License-Identifier: GPL-3.0-or-later
//! Content QA. Structural errors fail compilation; everything here is a
//! diagnostic an author (human or agent) can act on.

use serde::Serialize;
use std::collections::HashMap;

use crate::cite::{CiteOutcome, SourceCorpus};
use crate::model::{Graph, NodeKind, Pack};

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
        }
    }
}

/// Reachability from each pack start. Instances (`base@origin`) are entered
/// through links, not their start, so they are skipped.
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
        for (i, n) in g.nodes.iter().enumerate() {
            if n.pack == p.id && !seen[i] && !n.synthetic {
                d.push(Diagnostic {
                    severity: "warn",
                    code: "unreachable",
                    at: n.id.clone(),
                    message: format!("not reachable from {} start", p.id),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{RawNode, Source};

    fn pack_with_cite(sources: Vec<Source>, cite: &str) -> Pack {
        Pack {
            schema_version: 2,
            id: "test".into(),
            title: "Test".into(),
            description: None,
            jurisdiction: None,
            forum: None,
            start_node_id: "n1".into(),
            groups: vec![],
            roles: std::collections::BTreeMap::new(),
            sources,
            nodes: vec![RawNode {
                id: "n1".into(),
                label: "n1".into(),
                cite: Some(cite.to_string()),
                ..Default::default()
            }],
            edges: vec![],
        }
    }

    fn source(path: Option<&str>) -> Source {
        Source {
            id: "s".into(),
            title: None,
            url: None,
            path: path.map(str::to_string),
            sha256: None,
            as_of: None,
        }
    }

    fn frcp_corpus() -> SourceCorpus {
        SourceCorpus::from_files([(
            "sources/frcp-fixture.txt".to_string(),
            "## Rule 12\nDefenses and objections.\n".to_string(),
        )])
    }

    #[test]
    fn reports_unverifiable_cite_with_reason() {
        let p = pack_with_cite(vec![source(Some("sources/frcp-fixture.txt"))], "FRCP 99");
        let mut diags = vec![];
        let mut push = |severity, code, at: String, message: String| {
            diags.push(Diagnostic {
                severity,
                code,
                at,
                message,
            });
        };
        lint_pack_cites(&p, &frcp_corpus(), &mut push);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, "unverifiable-cite");
        assert_eq!(diags[0].severity, "warn");
        assert!(diags[0].message.contains("Rule 99"));
    }

    #[test]
    fn no_diagnostic_for_cite_that_resolves() {
        let p = pack_with_cite(vec![source(Some("sources/frcp-fixture.txt"))], "FRCP 12");
        let mut diags = vec![];
        let mut push = |severity, code, at: String, message: String| {
            diags.push(Diagnostic {
                severity,
                code,
                at,
                message,
            });
        };
        lint_pack_cites(&p, &frcp_corpus(), &mut push);
        assert!(diags.is_empty());
    }

    /// A pack with zero `sources` is already covered by `no-sources`
    /// (`lint_pack_sources`); `lint_pack_cites` must not pile on a
    /// duplicate per-cite diagnostic.
    #[test]
    fn no_duplicate_diagnostic_when_pack_has_no_sources() {
        let p = pack_with_cite(vec![], "FRCP 12(b)(6)");
        let mut diags = vec![];
        let mut push = |severity, code, at: String, message: String| {
            diags.push(Diagnostic {
                severity,
                code,
                at,
                message,
            });
        };
        lint_pack_cites(&p, &frcp_corpus(), &mut push);
        assert!(diags.is_empty());
    }

    #[test]
    fn case_citation_produces_no_diagnostic() {
        let p = pack_with_cite(
            vec![source(Some("sources/frcp-fixture.txt"))],
            "Bowles v. Russell, 551 U.S. 205 (2007)",
        );
        let mut diags = vec![];
        let mut push = |severity, code, at: String, message: String| {
            diags.push(Diagnostic {
                severity,
                code,
                at,
                message,
            });
        };
        lint_pack_cites(&p, &frcp_corpus(), &mut push);
        assert!(diags.is_empty());
    }

    /// End-to-end through the public `lint()` entry point (uses the real
    /// embedded corpus via `SourceCorpus::default_source()`), not just the
    /// unit-level `lint_pack_cites` helper.
    #[test]
    fn lint_reports_unverifiable_cite_via_public_entry_point() {
        use crate::model::{CompileOptions, Graph, LinkFile};
        let p = pack_with_cite(vec![source(Some("sources/frcp.txt"))], "FRCP 9999");
        let g = Graph::compile(
            std::slice::from_ref(&p),
            &LinkFile::default(),
            &CompileOptions::default(),
        )
        .unwrap();
        let diags = lint(&g, std::slice::from_ref(&p));
        assert!(diags.iter().any(|d| d.code == "unverifiable-cite"));
    }
}
