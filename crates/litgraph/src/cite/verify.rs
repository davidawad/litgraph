// SPDX-License-Identifier: GPL-3.0-or-later
//! Resolve a pack's `cite`/`authority` strings against its `sources`: split
//! and normalize (`normalize`), find the vendored files this pack actually
//! points at (`corpus`), and — if a quoted phrase is given in `note` —
//! fuzzy-match it within the resolved span (`fuzzy`). This is the mechanism
//! `lint::lint_pack_cites` reports diagnostics from.

use crate::cite::corpus::SourceCorpus;
use crate::cite::fuzzy::{self, DEFAULT_THRESHOLD};
use crate::cite::normalize::{parse_cite_string, CiteRef};
use crate::model::Pack;

/// What became of one normalized citation.
#[derive(Debug, Clone, PartialEq)]
pub enum CiteOutcome {
    /// Resolved to a span in a vendored source this pack declares.
    Verified {
        /// `Pack.sources[].path` of the file the span was found in.
        source_path: String,
        /// Fuzzy-match score of a quoted phrase in `note`, if one was
        /// given and checked. `None` means no quote was present to check
        /// (the cite still verified — it just resolved to a section, not
        /// a specific quoted span within it).
        quote_score: Option<f64>,
        /// The matched span of the vendored text, when `quote_score` is
        /// `Some` — lets a human/agent see exactly what the quote in
        /// `note` matched, not just that it scored above threshold.
        quote_snippet: Option<String>,
    },
    /// A case citation, or text this parser doesn't treat as a rule/statute
    /// cite at all — deliberately never checked against the L0 corpus.
    OutOfScope,
    /// Recognized as a rule/statute citation but could not be resolved;
    /// `reason` is the exact diagnostic text.
    Unresolvable {
        /// Human-readable, specific reason (which `lint` surfaces verbatim).
        reason: String,
    },
}

/// One citation found on one node/edge, and what became of it.
#[derive(Debug, Clone, PartialEq)]
pub struct CiteCheck {
    /// `<node-or-edge-id>` (without the pack prefix; the caller adds it).
    pub at: String,
    /// The raw `cite`/`authority` string this citation was parsed out of
    /// (may be compound — several [`CiteCheck`]s can share one `raw`).
    pub raw: String,
    /// The normalized citation.
    pub cite_ref: CiteRef,
    /// What became of it.
    pub outcome: CiteOutcome,
}

/// Check every `cite` (nodes) and `authority` (edges) in `pack` against
/// `corpus`, honoring only the sources `pack` itself declares
/// (`Pack.sources[].path`) — a cite is never "verified" against a vendored
/// file the pack doesn't actually cite as one of its own sources, even if
/// that file happens to be in the corpus.
#[must_use]
pub fn check_pack(pack: &Pack, corpus: &SourceCorpus) -> Vec<CiteCheck> {
    let vendored_paths = pack_vendored_paths(pack, corpus);
    let mut out = vec![];
    for n in &pack.nodes {
        let Some(cite) = &n.cite else { continue };
        out.extend(check_field(
            &n.id,
            cite,
            n.note.as_deref(),
            pack,
            corpus,
            &vendored_paths,
        ));
    }
    for e in &pack.edges {
        let Some(authority) = &e.authority else {
            continue;
        };
        let at =
            e.id.clone()
                .unwrap_or_else(|| format!("{}->{}", e.from, e.to));
        out.extend(check_field(
            &at,
            authority,
            e.note.as_deref(),
            pack,
            corpus,
            &vendored_paths,
        ));
    }
    out
}

/// `Pack.sources[].path` entries that both are set and actually exist in
/// `corpus` — the only files a cite in this pack may resolve against.
fn pack_vendored_paths(pack: &Pack, corpus: &SourceCorpus) -> Vec<String> {
    pack.sources
        .iter()
        .filter_map(|s| s.path.as_deref())
        .filter(|p| corpus.has_file(p))
        .map(str::to_string)
        .collect()
}

fn check_field(
    at: &str,
    raw: &str,
    note: Option<&str>,
    pack: &Pack,
    corpus: &SourceCorpus,
    vendored_paths: &[String],
) -> Vec<CiteCheck> {
    let quotes = note.map(extract_quotes).unwrap_or_default();
    parse_cite_string(raw, pack.forum.as_deref())
        .into_iter()
        .map(|cite_ref| {
            let outcome = resolve(pack, &cite_ref, corpus, vendored_paths, &quotes);
            CiteCheck {
                at: at.to_string(),
                raw: raw.to_string(),
                cite_ref,
                outcome,
            }
        })
        .collect()
}

fn resolve(
    pack: &Pack,
    cite_ref: &CiteRef,
    corpus: &SourceCorpus,
    vendored_paths: &[String],
    quotes: &[String],
) -> CiteOutcome {
    if cite_ref.is_out_of_scope() {
        return CiteOutcome::OutOfScope;
    }
    let Some(heading) = cite_ref.heading() else {
        return CiteOutcome::Unresolvable {
            reason: format!("`{}` did not normalize to a lookup key", cite_ref.raw),
        };
    };
    if pack.sources.is_empty() {
        return CiteOutcome::Unresolvable {
            reason: "pack has no `sources`; cite cannot be traced to a primary document".into(),
        };
    }
    if vendored_paths.is_empty() {
        return CiteOutcome::Unresolvable {
            reason: format!(
                "none of this pack's `sources` has a local `path` to vendored text (only `url` is set); \"{heading}\" cannot be checked offline"
            ),
        };
    }
    let hits: Vec<&(String, crate::cite::corpus::Section)> = corpus
        .lookup_all(&heading)
        .iter()
        .filter(|(path, _)| vendored_paths.contains(path))
        .collect();
    let Some((path, section)) = hits.first() else {
        return CiteOutcome::Unresolvable {
            reason: format!(
                "no vendored source in this pack's `sources` (checked: {}) contains a \"## {heading}\" section",
                vendored_paths.join(", ")
            ),
        };
    };
    if quotes.is_empty() {
        return CiteOutcome::Verified {
            source_path: path.clone(),
            quote_score: None,
            quote_snippet: None,
        };
    }
    for quote in quotes {
        if let Some(m) = fuzzy::fuzzy_find(&section.body, quote, DEFAULT_THRESHOLD) {
            return CiteOutcome::Verified {
                source_path: path.clone(),
                quote_score: Some(m.score),
                quote_snippet: Some(m.snippet),
            };
        }
    }
    CiteOutcome::Unresolvable {
        reason: format!(
            "quoted phrase in `note` not found (even fuzzily) within the \"{heading}\" span of {path}"
        ),
    }
}

/// Pull out every `"..."`/`“...”`-quoted phrase in `note` at least four
/// characters long (short quotes like `"e"` produce noise, not signal).
fn extract_quotes(note: &str) -> Vec<String> {
    let mut out = vec![];
    for (start, c) in note.char_indices() {
        let closer = match c {
            '"' => '"',
            '\u{201c}' => '\u{201d}',
            _ => continue,
        };
        if let Some(end) = note[start + c.len_utf8()..].find(closer) {
            let inner = &note[start + c.len_utf8()..start + c.len_utf8() + end];
            if inner.trim().len() >= 4 {
                out.push(inner.trim().to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{RawEdge, RawNode, Source};

    fn pack_with(sources: Vec<Source>, nodes: Vec<RawNode>, edges: Vec<RawEdge>) -> Pack {
        Pack {
            schema_version: 2,
            id: "test".into(),
            title: "Test".into(),
            description: None,
            jurisdiction: None,
            forum: None,
            start_node_id: nodes.first().map_or_else(|| "n1".into(), |n| n.id.clone()),
            groups: vec![],
            roles: std::collections::BTreeMap::new(),
            sources,
            nodes,
            edges,
        }
    }

    fn corpus_with_frcp12() -> SourceCorpus {
        SourceCorpus::from_files([(
            "sources/frcp-fixture.txt".to_string(),
            "## Rule 12\nRule 12 — Defenses and Objections\n\
             (b) How to present defenses. A party may assert failure to state a claim \
             upon which relief can be granted.\n"
                .to_string(),
        )])
    }

    fn source(id: &str, path: Option<&str>) -> Source {
        Source {
            id: id.into(),
            title: None,
            url: None,
            path: path.map(str::to_string),
            sha256: None,
            as_of: None,
        }
    }

    fn node_with_cite(id: &str, cite: &str, note: Option<&str>) -> RawNode {
        RawNode {
            id: id.into(),
            label: id.into(),
            cite: Some(cite.into()),
            note: note.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn verifies_cite_with_vendored_path() {
        let pack = pack_with(
            vec![source("frcp", Some("sources/frcp-fixture.txt"))],
            vec![node_with_cite("n1", "FRCP 12(b)(6)", None)],
            vec![],
        );
        let checks = check_pack(&pack, &corpus_with_frcp12());
        assert_eq!(checks.len(), 1);
        assert!(matches!(
            checks[0].outcome,
            CiteOutcome::Verified {
                quote_score: None,
                ..
            }
        ));
    }

    #[test]
    fn verifies_quoted_phrase_fuzzily() {
        let pack = pack_with(
            vec![source("frcp", Some("sources/frcp-fixture.txt"))],
            vec![node_with_cite(
                "n1",
                "FRCP 12(b)(6)",
                Some("permits dismissal for \"failure to state a claim upon which relief can be granted\""),
            )],
            vec![],
        );
        let checks = check_pack(&pack, &corpus_with_frcp12());
        assert_eq!(checks.len(), 1);
        match &checks[0].outcome {
            CiteOutcome::Verified {
                quote_score,
                quote_snippet,
                ..
            } => {
                assert_eq!(*quote_score, Some(1.0));
                // The snippet is a best-effort excerpt (see
                // `fuzzy::extract_snippet`'s doc comment), not guaranteed to
                // be byte-exact; it should still contain the matched text.
                assert!(quote_snippet
                    .as_deref()
                    .is_some_and(|s| s.contains("failure to state a claim")));
            }
            other => panic!("expected Verified, got {other:?}"),
        }
    }

    /// Title `0` (a degenerate parse of e.g. `"0 U.S.C. § 5"`) has no
    /// canonical heading to look up; `resolve` must fail closed rather than
    /// than panic on the lookup.
    #[test]
    fn unresolvable_when_cite_has_no_heading() {
        let pack = pack_with(
            vec![source("frcp", Some("sources/frcp-fixture.txt"))],
            vec![node_with_cite("n1", "0 U.S.C. § 5", None)],
            vec![],
        );
        let checks = check_pack(&pack, &corpus_with_frcp12());
        match &checks[0].outcome {
            CiteOutcome::Unresolvable { reason } => assert!(reason.contains("lookup key")),
            other => panic!("expected Unresolvable, got {other:?}"),
        }
    }

    #[test]
    fn unresolvable_when_quote_not_in_span() {
        let pack = pack_with(
            vec![source("frcp", Some("sources/frcp-fixture.txt"))],
            vec![node_with_cite(
                "n1",
                "FRCP 12(b)(6)",
                Some("requires \"a demand for the relief sought\" in every pleading"),
            )],
            vec![],
        );
        let checks = check_pack(&pack, &corpus_with_frcp12());
        assert_eq!(checks.len(), 1);
        match &checks[0].outcome {
            CiteOutcome::Unresolvable { reason } => assert!(reason.contains("not found")),
            other => panic!("expected Unresolvable, got {other:?}"),
        }
    }

    #[test]
    fn unresolvable_when_pack_has_no_sources() {
        let pack = pack_with(
            vec![],
            vec![node_with_cite("n1", "FRCP 12(b)(6)", None)],
            vec![],
        );
        let checks = check_pack(&pack, &corpus_with_frcp12());
        match &checks[0].outcome {
            CiteOutcome::Unresolvable { reason } => assert!(reason.contains("no `sources`")),
            other => panic!("expected Unresolvable, got {other:?}"),
        }
    }

    #[test]
    fn unresolvable_when_source_has_no_local_path() {
        let pack = pack_with(
            vec![source("frcp", None)], // url-only, like cafc/cofc today
            vec![node_with_cite("n1", "FRCP 12(b)(6)", None)],
            vec![],
        );
        let checks = check_pack(&pack, &corpus_with_frcp12());
        match &checks[0].outcome {
            CiteOutcome::Unresolvable { reason } => assert!(reason.contains("local `path`")),
            other => panic!("expected Unresolvable, got {other:?}"),
        }
    }

    #[test]
    fn unresolvable_when_heading_missing_from_vendored_file() {
        let pack = pack_with(
            vec![source("frcp", Some("sources/frcp-fixture.txt"))],
            vec![node_with_cite("n1", "FRCP 99", None)],
            vec![],
        );
        let checks = check_pack(&pack, &corpus_with_frcp12());
        match &checks[0].outcome {
            CiteOutcome::Unresolvable { reason } => assert!(reason.contains("Rule 99")),
            other => panic!("expected Unresolvable, got {other:?}"),
        }
    }

    #[test]
    fn case_citation_on_edge_is_out_of_scope() {
        let pack = pack_with(
            vec![source("frcp", Some("sources/frcp-fixture.txt"))],
            vec![],
            vec![RawEdge {
                from: "a".into(),
                to: "b".into(),
                label: "l".into(),
                authority: Some("Bowles v. Russell, 551 U.S. 205 (2007)".into()),
                ..Default::default()
            }],
        );
        let checks = check_pack(&pack, &corpus_with_frcp12());
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].outcome, CiteOutcome::OutOfScope);
    }

    #[test]
    fn compound_cite_produces_one_check_per_sub_citation() {
        let pack = pack_with(
            vec![source("frcp", Some("sources/frcp-fixture.txt"))],
            vec![node_with_cite("n1", "FRCP 12(b)(6); FRCP 99", None)],
            vec![],
        );
        let checks = check_pack(&pack, &corpus_with_frcp12());
        assert_eq!(checks.len(), 2);
        assert!(matches!(checks[0].outcome, CiteOutcome::Verified { .. }));
        assert!(matches!(
            checks[1].outcome,
            CiteOutcome::Unresolvable { .. }
        ));
    }

    #[test]
    fn extract_quotes_ignores_short_fragments() {
        assert_eq!(
            extract_quotes(r#"the term "or" is used loosely"#),
            Vec::<String>::new()
        );
        assert_eq!(
            extract_quotes(r#"quoting "a short phrase" here"#),
            vec!["a short phrase".to_string()]
        );
    }

    #[test]
    fn extract_quotes_handles_curly_quotes() {
        assert_eq!(
            extract_quotes("the rule says \u{201c}within 21 days\u{201d} of service"),
            vec!["within 21 days".to_string()]
        );
    }

    #[test]
    fn edge_without_authority_and_node_without_cite_produce_no_checks() {
        let pack = pack_with(
            vec![],
            vec![RawNode {
                id: "n1".into(),
                label: "n1".into(),
                ..Default::default()
            }],
            vec![RawEdge {
                from: "n1".into(),
                to: "n1".into(),
                label: "l".into(),
                ..Default::default()
            }],
        );
        assert!(check_pack(&pack, &corpus_with_frcp12()).is_empty());
    }
}
