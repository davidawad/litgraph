// SPDX-License-Identifier: GPL-3.0-or-later
//! Cite verification against L0 sources (`docs/ARCHITECTURE.md`'s tower).
//!
//! `litgraph lint` should be able to tell an author (human or agent) not
//! just "this edge has an authority cite" but "that cite is checkable, and
//! here it is in the primary text" — or, if not, exactly why not. Three
//! pieces:
//!
//! - [`normalize`]: turn "FRCP 12(b)(6)", "Fed. R. Civ. P. 12(b)(6)",
//!   "28 U.S.C. § 1498(a)", "35 USC 315(e)", "37 C.F.R. § 42.108", "RCFC
//!   56", ... into one canonical shape, splitting compound citations
//!   ("28 U.S.C. 1291; 2106") and recognizing case citations as
//!   out-of-scope rather than malformed rule cites.
//! - [`corpus`]: the vendored plain-text L0 sources (`sources/*.txt`,
//!   embedded the same way `packs/*.json` is) that a pack's own
//!   `sources[].path` points into.
//! - [`fuzzy`]: match a quoted phrase from a pack's `note` against the
//!   resolved span, tolerant of minor whitespace/punctuation drift.
//!
//! [`verify::check_pack`] ties them together; `lint::lint_pack_cites`
//! turns the result into `Diagnostic`s.

pub mod corpus;
pub mod fuzzy;
pub mod normalize;
pub mod verify;

pub use corpus::SourceCorpus;
pub use normalize::{parse_cite_string, CiteRef, Family};
pub use verify::{check_pack, CiteCheck, CiteOutcome};
