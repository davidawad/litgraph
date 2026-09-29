# L0 source corpus — provenance

These files are the **L0 layer** in `docs/ARCHITECTURE.md`'s tower: verbatim
plain-text extracts of primary law (rule texts, statutes, regulations), each
with a retrieval date and a sha256 recorded below so the extraction is
auditable. `litgraph lint` fuzzy-matches pack `cite`/`authority` strings (and
quoted phrases in `note`) against these files once a pack's `sources[].path`
points at one (see `docs/PACK_SCHEMA.md` and `docs/CITES.md`).

## Why vendor plain text instead of fetching at run/test time

- **Public domain.** Every document here is a U.S. government work (court
  rules prescribed under 28 U.S.C. § 2072, the U.S. Code, the Code of Federal
  Regulations) — no copyright restriction on redistributing the text.
- **Small.** The whole corpus is under 750 KB uncompressed — a rounding error
  next to the packs it backs, and far smaller than vendoring full PDFs.
- **Deterministic, offline tests.** `cargo nextest run` must not need network
  access (repo quality bar). A `litgraph sources fetch` command hitting
  live .gov/Cornell endpoints in CI would be slow and flaky (rate limits,
  layout changes, outages) for content that changes on the order of years,
  not days. Vendoring converts "fetch reliability" into an ordinary content
  review, the same way `packs/*.json` themselves are reviewed.
- **Matches how packs are already authored.** `packs/cafc-SOURCES-NOTES.md`
  and `packs/cofc-SOURCES-NOTES.md` document a `pdftotext -layout` +
  sha256 authoring pass already; this corpus is the same pattern, just
  checked into the repo instead of staying in a private KB so cite
  verification can run in CI.

Re-deriving any file here (to refresh after a rule amendment) means
re-running the extraction described in its entry below and diffing the
result — that is the "reproducible" bar, not a live network call at lint
time.

## Format

Each `.txt` file is one or more sections, each starting at column 0 with
`## <canonical ref>` (matching the ref strings `litgraph::cite::normalize`
produces — see `crates/litgraph/src/cite/normalize.rs`), optionally followed
by a heading line, then the verbatim body text up to the next `## ` line.

## Files

### `frcp.txt` — Federal Rules of Civil Procedure (112 rules)

- Ported from `civ-pro-the-gathering/content/reference/frcp-full.json`
  (sibling project, same author, also public-domain rule text with its own
  provenance block: "official 2024 committee print... extracted locally
  (pdftotext) from David's library copy", retrieved 2026-07-26, canonical
  `https://www.uscourts.gov/rules-policies/current-rules-practice-procedure`).
  Re-formatted into this file's `## Rule N` section convention; no wording
  changed.
- sha256 (this file): `cb9a19a0ad99acdbd04f46acd3893bb96fbd34c9a8f66de57f5d171dbd268b44`

### `frap.txt` — Federal Rules of Appellate Procedure (55 rules)

- Ported from `civ-pro-the-gathering/content/reference/frap-full.json`,
  whose provenance records each rule "extracted from the Cornell LII...
  pages and then verified line by line against the official Administrative
  Office edition (uscourts.gov, December 1, 2024)" (Rules 6/39 verified
  against the Apr. 23, 2025 Supreme Court amendment order instead),
  retrieved 2026-08-15.
- sha256 (this file): `87d71a1f443c5c084cc9f11932a66436ab300f83146ee2c5ce2654a8a4824479`

### `usc-cfr-prosecution-fragments.txt` — 28 U.S.C. (4 sections), 35 U.S.C.
(7 sections), 37 C.F.R. Part 1 prosecution rules (17 sections)

- Ported from `civ-pro-the-gathering/content/reference/uscfr-full.json`.
  35 U.S.C./37 C.F.R. text there was "extracted locally from the USPTO
  Consolidated Patent Laws/Rules (July 2025)"; the 28 U.S.C. sections were
  "appended from uscode.house.gov (official)" and, in the source JSON,
  embedded a full uscode.house.gov page dump (edition-history sidebar,
  nav chrome) around the actual statute text. This port strips that
  boilerplate mechanically: keeps only the span from the `§NNNN. Title`
  heading line to the `[Excerpt ...]` marker the JSON's own author added.
- sha256 (this file, post-cleanup): `5bbba22c27cd4f0957c799a8a1f48b6ba9d0670be03a9fec796a3c8ec394c1b3`

### `usc-19-1337.txt` — 19 U.S.C. § 1337 (ITC unfair-import-practices statute)

- Fetched directly 2026-09-28 from `uscode.house.gov`
  (`https://uscode.house.gov/view.xhtml?req=(title:19%20section:1337%20edition:prelim)`,
  official U.S. House Office of the Law Revision Counsel text), full text
  including subsections (a)-(n). HTML chrome/nav stripped mechanically
  (kept the span from the `§1337.` heading to the `Editorial Notes`
  marker), tags stripped, entities decoded.
- sha256 (this file): `3865d7c3c1b4d9c0883d3eb29fd160c3c789d8cb8ffed675311e5dccd0892408`

### `cfr-37-part42.txt` — 37 C.F.R. Part 42 (PTAB trial practice, 84 sections)

- Fetched directly 2026-09-28 from `www.govinfo.gov`'s official annual CFR
  XML (`CFR-2024-title37-vol1-part42.xml`, title 37 "Patents, Trademarks,
  and Copyrights", 2024-07-01 edition), the authoritative machine-readable
  form GPO publishes for the printed CFR. Parsed with Python's
  `xml.etree.ElementTree`: every `<SECTION>` becomes one `## 37 C.F.R. §
  N` block from its `<SECTNO>`/`<SUBJECT>`/`<P>` children, whitespace
  collapsed, tags stripped. Covers `§ 42.108` (institution of *inter
  partes* review) cited as the worked example in this bead's brief.
- sha256 (this file): `06786d129e12353754288989ddad0ef9866bb068eb0a7a35e25465b65276e4a1`

## Deliberately not vendored yet (follow-up)

- **RCFC** (Rules of the U.S. Court of Federal Claims) and the **Federal
  Circuit's local rules / IOPs** — both PDF-only at their official sources
  (`uscfc.uscourts.gov`, `cafc.uscourts.gov`); no clean HTML/XML mirror
  found during this pass. `packs/cofc-SOURCES-NOTES.md` and
  `packs/cafc-SOURCES-NOTES.md` already record the specific PDFs + sha256
  used to author those two packs; a follow-up pass should `pdftotext
  -layout` those same PDFs and vendor the result here rather than
  re-fetching from scratch.
- **Fed. R. Crim. P.** — table of contents confirmed available (Cornell
  LII), full text not pulled in this pass; low priority since
  `frcrimp-criminal-procedure.json` is one of the six packs another agent
  is actively migrating in parallel (see the follow-up bead).
- **MPEP** chapters 600/700/1200/1300 — `civ-pro-the-gathering`'s
  `content/reference/mpep-sections.json` already has all 456 sections
  ported from David's local MPEP conversion; not pulled in here only
  because of size (≈940 KB JSON) relative to this pass's time budget, not
  because of any sourcing problem. Straightforward to port with the same
  script used for `frcp.txt`/`frap.txt` above.
- **19 C.F.R. Part 210** (ITC's own procedural rules — `itc-337.json`
  cites these via an internal `cfr210.N.x` shorthand) — not fetched.
