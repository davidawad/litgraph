// SPDX-License-Identifier: GPL-3.0-or-later
//! Reports cite-verification coverage across every embedded pack: how many
//! `cite`/`authority` citations parse, how many are out-of-scope (case law,
//! doctrinal shorthand), and — of the rest — how many actually resolve
//! against the vendored L0 corpus (`sources/*.txt`) versus why not. Run
//! with `cargo test --test cite_coverage_report -- --nocapture` for the
//! full per-pack breakdown; the asserts below are weak sanity checks (this
//! is a report, not a correctness gate — `lint`'s own tests cover
//! correctness).
//!
//! Two views are printed, because they answer different questions:
//!
//! 1. **Strict** (what `litgraph lint` actually reports): a cite only
//!    verifies if its pack's own `sources[].path` points at vendored text.
//!    As of the lg-hc9 wiring pass, every pack's `sources[].path` is wired to
//!    the corpus it needs (see `sources/PROVENANCE.md`), so this is now
//!    ~88% of all citations (~99% of in-scope ones) — up from 0 before that
//!    pass, which is the state `docs/PACK_SCHEMA.md` originally described.
//! 2. **Corpus reach** (diagnostic only, ignoring `sources[].path`): of the
//!    citations that normalize to a family/section, how many have a
//!    matching `## <heading>` anywhere in the vendored corpus. This is what
//!    vendoring more text would unlock; the residual `strict_unresolvable`
//!    citations are now a small, individually-understood set — see
//!    `sources/PROVENANCE.md`'s "Deliberately not vendored yet" (a handful of
//!    MPEP sections whose body text this pass couldn't cleanly isolate from
//!    a two-column PDF layout) plus a few `note` fields whose quoted phrase
//!    is a genuine paraphrase of the source rather than a verbatim quote.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use litgraph::api::Catalog;
use litgraph::cite::{self, CiteOutcome, SourceCorpus};

#[test]
fn report_cite_verification_across_all_embedded_packs() {
    let catalog = Catalog::embedded().expect("embedded catalog loads");
    let corpus = SourceCorpus::embedded();

    let mut strict_verified = 0usize;
    let mut strict_unresolvable = 0usize;
    let mut out_of_scope = 0usize;
    let mut corpus_reach_hits = 0usize;
    let mut corpus_reach_misses = 0usize;
    let mut per_pack: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new(); // (total, out_of_scope, strict_verified)

    for (_, pack, _) in &catalog.packs {
        let checks = cite::check_pack(pack, &corpus);
        let entry = per_pack.entry(pack.id.clone()).or_default();
        entry.0 += checks.len();

        for check in &checks {
            match &check.outcome {
                CiteOutcome::Verified { .. } => {
                    strict_verified += 1;
                    entry.2 += 1;
                }
                CiteOutcome::OutOfScope => {
                    out_of_scope += 1;
                    entry.1 += 1;
                }
                CiteOutcome::Unresolvable { .. } => {
                    strict_unresolvable += 1;
                    // Corpus-reach view: does *some* vendored file (any
                    // pack's, not just this one's declared `sources`) have
                    // this heading at all?
                    if let Some(heading) = check.cite_ref.heading() {
                        if corpus.lookup_all(&heading).is_empty() {
                            corpus_reach_misses += 1;
                        } else {
                            corpus_reach_hits += 1;
                        }
                    } else {
                        corpus_reach_misses += 1;
                    }
                }
            }
        }
    }

    let total = strict_verified + strict_unresolvable + out_of_scope;

    println!(
        "\n=== Cite verification coverage across {total} citations in {} packs ===",
        per_pack.len()
    );
    println!(
        "strict (litgraph lint today): {strict_verified} verified, {strict_unresolvable} unresolvable, {out_of_scope} out-of-scope (case law/other)"
    );
    println!(
        "corpus reach (diagnostic — ignores sources[].path wiring): {corpus_reach_hits}/{} unresolvable citations have a matching heading somewhere in the vendored corpus",
        corpus_reach_hits + corpus_reach_misses
    );
    println!("\nper-pack (total citations, out-of-scope, strict-verified):");
    for (pack_id, (n, oos, verified)) in &per_pack {
        println!(
            "  {pack_id:<10} {n:>4} total, {oos:>3} out-of-scope, {verified:>3} strict-verified"
        );
    }

    // Weak sanity checks — this test's job is to print the report above,
    // not to gate the build on a specific count.
    assert!(
        total > 200,
        "expected several hundred citations across all packs, got {total}"
    );
    assert!(
        strict_verified > 1400,
        "every pack's sources[].path is wired to the corpus it needs (see sources/PROVENANCE.md) as \
         of the lg-hc9 pass — if this drops well below where it landed there, something got unwired \
         or a vendored file went missing; update this comment and the number if the corpus \
         legitimately shrinks (e.g. a pack is removed)"
    );
    assert!(
        strict_unresolvable < 50,
        "residual unresolvable citations should stay a small, individually-explained set (see \
         sources/PROVENANCE.md's \"Deliberately not vendored yet\") — got {strict_unresolvable}; if \
         this grew, a newly-added cite/authority string or pack needs wiring, not a higher threshold"
    );
    // `corpus_reach_hits` (printed above) was a proxy for "low-hanging fruit
    // the next wiring pass should pick up" back when strict_unresolvable was
    // in the hundreds. Now that every easy win is wired (see
    // sources/PROVENANCE.md), 0 is the success state, not a red flag — every
    // reach hit left over would BE a wiring bug, not an opportunity — so
    // there's no lower-bound assertion on it here.
}

/// Audit tool, not a gate: for the families this bead vendored the *whole*
/// text of (FRCP, FRAP, 19 U.S.C. § 1337, 37 C.F.R. Part 42 — as opposed to
/// families like "37 C.F.R. Part 1" where only some sections are vendored),
/// a corpus miss is a much stronger "clearly wrong cite" signal than for a
/// partially-vendored family, since there's no "just not vendored yet"
/// explanation available. Run with `--ignored --nocapture` when reviewing a
/// pack; used during this bead's own review (see the final report) and
/// found only citation-range forms ("FRAP 28-31") the normalizer doesn't
/// decompose yet, not actual wrong rule numbers.
#[test]
#[ignore = "audit tool, run on demand with --ignored --nocapture; not part of the default suite"]
fn list_misses_in_fully_vendored_families() {
    let catalog = Catalog::embedded().expect("embedded catalog loads");
    let corpus = SourceCorpus::embedded();
    for (_, pack, _) in &catalog.packs {
        for check in cite::check_pack(pack, &corpus) {
            let fully_vendored = matches!(
                check.cite_ref.family,
                cite::Family::Frcp
                    | cite::Family::Frap
                    | cite::Family::Usc(19)
                    | cite::Family::Cfr(37)
            );
            if !fully_vendored {
                continue;
            }
            let Some(heading) = check.cite_ref.heading() else {
                continue;
            };
            if corpus.lookup_all(&heading).is_empty() {
                println!(
                    "{} :: {} -> raw={:?} heading={:?}",
                    pack.id, check.at, check.raw, heading
                );
            }
        }
    }
}
