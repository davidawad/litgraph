# L0 source corpus — provenance

These files are the **L0 layer** in `docs/ARCHITECTURE.md`'s tower: verbatim
plain-text extracts of primary law (rule texts, statutes, regulations), each
with a retrieval date and a sha256 recorded below so the extraction is
auditable. `litgraph lint` fuzzy-matches pack `cite`/`authority` strings (and
quoted phrases in `note`) against these files once a pack's `sources[].path`
points at one (see `docs/PACK_SCHEMA.md`'s authoring rule 1 and AGENTS.md's
pack-edit rule 4 for the mechanism; every pack's `sources[].path` is wired as
of this file's own entries below).

## Why vendor plain text instead of fetching at run/test time

- **Public domain.** Every document here is a U.S. government work (court
  rules prescribed under 28 U.S.C. § 2072, the U.S. Code, the Code of Federal
  Regulations) — no copyright restriction on redistributing the text.
- **Small.** The whole corpus is under 4 MB uncompressed (MPEP's 462 sections
  are most of that) — still a rounding error next to vendoring full PDFs,
  and small enough to embed in the binary the same way `packs/*.json` is.
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

### `rcfc.txt` — Rules of the U.S. Court of Federal Claims (112 rules, RCFC 1-87)

- The RCFC main-rules PDF is the same one `packs/cofc-court-of-federal-claims.json`'s
  `rcfc-rules` source already pins (`Rules%207.27.2026.pdf`,
  `www.uscfc.uscourts.gov`, as amended through July 27, 2026); re-fetched
  2026-09-29 and confirmed byte-identical (same sha256 as the pack's own
  record: `ff02c342f055be5e5f51df4de1dcec3c1264bb9907895d379d5baa796ec30a97`).
  `pdftotext` (no `-layout`; the two-column committee-notes sidebar in
  `-layout` mode interleaves with rule text mid-sentence, `pdftotext`'s
  content-stream reading order does not) against the main-rules window only
  (`Rule 1. Scope and Purpose` through `Rule 87`, before Appendix A/C/H/J and
  the Vaccine/Patent Rules — which restart their own `Rule 1..N` numbering
  under headings this corpus does not distinguish from the main rules — begin).
  Split into `## RCFC N` sections on a `^Rule N\.` line start (stripping the
  form-feed byte `pdftotext` emits at the top of each page first — several
  rules that happen to start a page, including RCFC 41, were silently
  dropped before this fix since the form-feed sat before `Rule` at column
  0). Committee-notes prose sometimes bleeds into the tail of the preceding
  rule's body (a known, accepted imprecision — it only affects optional
  quoted-phrase fuzzy matching, never whether a citation resolves at all).
- sha256 (this file): `aefd134e00b71dad05886f59cb11479e6381bbf1d5ad7a2c8e63191629761daf`

### `fedcir-rules.txt` — Federal Circuit Rules of Practice (55 numbered local rules)

- Same PDF `packs/cafc-federal-circuit.json`'s `fedcir-rules` source already
  pins (`FederalCircuitRulesofPractice.pdf`, `www.cafc.uscourts.gov`,
  December 1, 2025 edition); re-fetched 2026-09-29, confirmed byte-identical
  (sha256 `b6346ed56a0ef66e8a681765873caf69239cda273f8946f1625571e013712c05`
  matches the pack's own record). `pdftotext` (no `-layout`). This PDF
  combines FRAP-numbered local rules, Attorney Discipline Rules, and
  practice notes in one document; only the `FEDERAL CIRCUIT RULE N` running
  page headers are extracted (Attorney Discipline Rules and IOPs are a
  separate numbering scheme no pack currently cites). A rule spanning
  several pages repeats its own running header each page; those repeats are
  merged into one section rather than treated as a new one.
- sha256 (this file): `f38803d4f0370b737c9fcc85f2013b757a6a7648609637c88e652322de75b1a9`

### `cfr-19-part210.txt` — 19 C.F.R. Part 210 (USITC Rules of Adjudication and Enforcement, 79 sections)

- Fetched 2026-09-29 from `www.govinfo.gov`'s official annual CFR XML
  (`CFR-2024-title19-vol3-part210.xml`, title 19 "Customs Duties", chapter
  II "United States International Trade Commission", 2024-04-01 edition),
  same method and same `<SECTION>`/`<SECTNO>`/`<SUBJECT>`/`<P>` parse as
  `cfr-37-part42.txt` below. Resolves `itc-337.json`'s internal
  `cfr210.N.x` shorthand (see `crates/litgraph/src/cite/normalize.rs`'s
  `try_itc_cfr_slug`), which needs the pack's `forum: "itc"` field to
  disambiguate — set as part of this same pass.
- sha256 (this file): `92d29ab6a95dc52eaaa5b4fa5b1d0991131546e01612bc83f8e365f40420c99e`

### `cfr-37-part1.txt` — 37 C.F.R. Part 1 (Rules of Practice in Patent Cases, all 363 sections)

- Fetched 2026-09-29 from `www.govinfo.gov` (`CFR-2024-title37-vol1-part1.xml`,
  2024-07-01 edition), same method as `cfr-37-part42.txt`. Supersedes the
  17-section Part 1 subset already embedded piecemeal in
  `usc-cfr-prosecution-fragments.txt` (both are kept; a citation resolves
  against whichever file a given pack's `sources[].path` lists, so there is
  no conflict — see that test in `corpus.rs`, `duplicate_headings_across_files_both_appear`).
- sha256 (this file): `e5765fec0e8c5f295d4f230ed9743b7972dbd41340693833abf990fe1918d1ec`

### `cfr-37-part41.txt` — 37 C.F.R. Part 41 (Board of Patent Appeals and Interferences / PTAB ex parte appeal practice, 75 sections)

- Fetched 2026-09-29 from `www.govinfo.gov` (`CFR-2024-title37-vol1-part41.xml`,
  2024-07-01 edition), same method as `cfr-37-part42.txt`/`cfr-19-part210.txt`.
  Resolves `mpep-prosecution.json`'s citations into the examiner's-answer/
  appeal-to-PTAB region (§§ 41.20, 41.45).
- sha256 (this file): `912ba8c173b834a4030c979c0ff3bc6f96d393dcff717c6d14812daba23934ee`

### `frcrimp.txt` — Federal Rules of Criminal Procedure (62 rules)

- Fetched 2026-09-29 from `www.uscourts.gov`'s current-rules page
  (`federal-rules-of-criminal-procedure-dec-1-2024_0.pdf`, as amended to
  Dec. 1, 2024 — the same PDF `packs/frcrimp-criminal-procedure.json`'s
  `frcrimp-rules` source pins; sha256
  `c89161b8a258b62af4d5298de832c43bd72f2eea1104c7625a5b561658e5875c`
  confirmed identical). `pdftotext` (no `-layout`); this PDF has no
  appendices, so the whole body after the table of contents is main-rules
  text. Split on `^Rule N\.` line starts (form-feed byte stripped first,
  same fix as `rcfc.txt` above), page running headers/footers stripped
  mechanically.
- sha256 (this file): `cf92787ff6bf24cb4d853cb347925f5f37dba41c1bc1cac26da0ffcfb6cf19ed`

### `mpep.txt` — MPEP chapters 200 (partial), 600, 700, 800 (partial), 1200, 1300, 1400 (partial), 2200 (partial) — 466 sections

- Chapters 600/700/1200/1300 (456 sections) ported from
  `civ-pro-the-gathering/content/reference/mpep-sections.json` (sibling
  project; that file's own provenance: "Extracted locally from David's
  library conversion of the official USPTO MPEP", canonical
  `https://www.uspto.gov/web/offices/pac/mpep/`, retrieved 2026-07-26),
  reformatted into this file's `## MPEP § N` convention (that JSON's own
  `ref` field, e.g. `"MPEP 601"`, becomes heading `"MPEP § 601"` to match
  `normalize.rs`'s `Family::Mpep` heading form) — no wording changed.
- §§ 1401/1412/1460 (chapter 1400, reissue) and §§ 2209/2249/2254 (chapter
  2200, ex parte reexam) — 6 sections `mpep-prosecution.json` cites that the
  sibling repo's JSON doesn't cover — fetched 2026-09-29 directly from
  `www.uspto.gov` (`mpep-1400.pdf`, `mpep-2200.pdf`, 9th ed. Rev. 01.2024).
- §§ 201.06/201.07 (chapter 200) and §§ 818.01/821.04 (chapter 800) — 4 more
  sections the same pack cites — fetched the same way from `mpep-0200.pdf`/
  `mpep-0800.pdf`.
- All 10 of the above: `pdftotext -layout`, sections located by line search
  and extracted as bounded windows around each heading. Lower extraction
  precision than the JSON-ported chapters (the two-column committee-note-
  style sidebar this edition uses bleeds into the window at points, and one
  section — MPEP § 803 — could not be cleanly isolated this way at all, so
  it stays unvendored; see "Deliberately not vendored yet" below); the
  imprecision is acceptable for the same reason as `rcfc.txt` above — it
  only affects optional quote-fuzzy-matching, never whether a citation
  resolves at all.
- Two `mpep-prosecution.json` citations (`MPEP 818.03`, `MPEP 818.03(a)`)
  were corrected to `MPEP 818.01`/`MPEP 818.01(d)` in the same pass: § 818.03
  does not exist in the current (9th ed., Rev. 01.2024) MPEP — confirmed by
  reading the actual chapter 800 text end to end looking for it — and the
  content those two edges describe (electing without/with traverse of a
  restriction requirement) is exactly § 818.01's own subject matter (which
  does exist, with an `(d) Traverse of Restriction` subsection).
- sha256 (this file): `110b56e793d244d0f16d1f9f00afe18a1f3d6fe2338daba8e000fc297fe3d524`

### `usc-additional.txt` — 57 more 28/35/18/5 U.S.C. sections

- The sections every pack's `sources` cite beyond the four 28 U.S.C. and
  seven 35 U.S.C. sections `usc-cfr-prosecution-fragments.txt` already had:
  28 U.S.C. §§ 46, 1254, 1291, 1295, 1331, 1332, 1391, 1491, 1498, 1500,
  1914, 2101, 2106, 2107, 2412, 2501, 2522; 35 U.S.C. §§ 6, 120, 121, 134,
  142, 143, 144, 154, 251, 286, 302-306, 307, 311, 312, 313-319, 321, 322,
  323-328; 18 U.S.C. §§ 3142, 3161, 3162, 3282, 3501, 3552, 3553; 5 U.S.C.
  §§ 556, 706. Fetched 2026-09-29 directly from
  `uscode.house.gov`'s per-section XHTML view (official U.S. House Office
  of the Law Revision Counsel text, `view.xhtml?req=(title:T section:S
  edition:prelim)`), the same official source `usc-19-1337.txt` used.
  Extracted mechanically between that page's own `<!-- field-start:head
  -->`/`<!-- field-end:head -->` and `<!-- field-start:statute -->`/`<!--
  field-end:statute -->` HTML comment markers (a machine-readable
  boundary the page itself provides, cleaner than the boilerplate-stripping
  `usc-cfr-prosecution-fragments.txt` needed), tags stripped, entities
  decoded.
- sha256 (this file): `5dedd90ad2e3e67185a2ae4b1ddfa68f7d7695b092666521fb9478c5c23852d8`

## Deliberately not vendored yet (follow-up)

- **RCFC Appendices A/C/H/J** (case management, bid-protest, ADR, patent
  rules) and the **Federal Circuit's Attorney Discipline Rules / Internal
  Operating Procedures / Mediation Guidelines** — no pack currently cites
  into these; `rcfc.txt`/`fedcir-rules.txt` above deliberately stop at the
  main numbered rules.
- **MPEP §§ 803 and 818** (top-level headings only — "Restriction — When
  Proper" and "Election and Reply") — cited by `mpep-prosecution.json`, but
  neither's own top-level body heading could be cleanly isolated in
  `mpep-0800.pdf`'s `pdftotext -layout` output in this pass (the two-column
  layout mixes each into surrounding statutory text without a clean `^803 `/
  `^818 ` line start the way their own lettered/decimal subsections have);
  §§ 818.01/821.04 from the same chapter *were* isolated cleanly and are in
  `mpep.txt`. A follow-up should try `pdftotext` without `-layout` (the
  approach that worked for `rcfc.txt`/`frcrimp.txt`) instead.
- **MPEP §§ 2210-2214** (ex parte reexam request formalities, cited as the
  range "MPEP 2209-2214" alongside § 2209, which *is* vendored) — same
  two-column extraction problem as § 803 above; only § 2209 and § 2254 from
  that chapter were cleanly isolated in this pass.
- **MPEP** chapters 2600 (inter partes reexam) and 2800 (supplemental
  examination) — not cited by any pack today.
- **Fed. R. Evid.** — no pack cites an FRE rule number today (only the
  `Daubert` case name, which is out-of-scope case law, not a rule cite);
  `normalize.rs` doesn't even have an FRE `Family` variant yet.
- Court/tribunal **fee schedules** and **statistical datapacks** (e.g.
  `uscfc-fee-schedule`, `cafc-fee-schedule`, `patentlyo-2024-datapack`,
  `ptab-tpg`) — these back a pack's dollar/probability figures, not a
  `cite`/`authority` string, so cite verification has no reason to vendor
  them; out of this corpus's scope entirely.
