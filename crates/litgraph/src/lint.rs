//! Content QA. Structural errors fail compilation; everything here is a
//! diagnostic an author (human or agent) can act on.

use serde::Serialize;
use std::collections::HashMap;

use crate::model::{Graph, NodeKind, Pack};

#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub severity: &'static str,
    pub code: &'static str,
    pub at: String,
    pub message: String,
}

pub fn lint(g: &Graph, packs: &[Pack]) -> Vec<Diagnostic> {
    let mut d = vec![];
    let mut push = |severity, code, at: String, message: String| {
        d.push(Diagnostic {
            severity,
            code,
            at,
            message,
        })
    };
    for p in packs {
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
        if p.sources.is_empty() {
            push(
                "warn",
                "no-sources",
                pid.clone(),
                "no `sources`; cites cannot be traced to a primary document".into(),
            );
        }
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
                push("warn", "parallel-edge-no-id", at.clone(), format!("parallel edge `{}` has no stable id (v1 keyed Q-values by from→to and silently collided here)", e.label));
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
                    let s: f64 = outs.iter().map(|e| e.probability.unwrap()).sum();
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
    // Reachability from each pack start.
    // Instances (`base@origin`) are entered through links, not their start.
    for p in g.packs.iter().filter(|p| !p.id.contains('@')) {
        let s = g.node(&p.start).unwrap();
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
    d.sort_by_key(|x| match x.severity {
        "error" => 0,
        "warn" => 1,
        _ => 2,
    });
    d
}
