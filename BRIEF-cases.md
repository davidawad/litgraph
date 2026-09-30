# Brief: famous-case expect tests + clearer tests

Goal: tests a lawyer can read, anchored in real, well-known procedural histories.

1. Pick 8-12 famous U.S. cases whose procedural path runs through forums
   litgraph models, for example: Bell Atlantic v. Twombly and Ashcroft v. Iqbal
   (FRCP 12(b)(6)), Celotex v. Catrett (summary judgment), eBay v. MercExchange
   (district court -> CAFC -> cert), Oil States v. Greene's Energy and SAS
   Institute v. Iancu (IPR/PTAB -> CAFC -> cert), United States v. Arthrex
   (PTAB Director review), a 28 U.S.C. 1498 CoFC patent case (e.g. Hughes
   Aircraft Co. v. United States), a CoFC bid protest, and an ITC 337 case
   reaching the Federal Circuit. Research each case's actual procedural history
   from primary or reputable sources (Justia, CourtListener, Oyez, the opinions
   themselves) and record citations.
2. For each case add `tests/cases/<slug>.json`: the pack(s), the scenario, the
   ordered list of real procedural events mapped to node/edge ids, and sources.
   A new integration test `tests/famous_cases.rs` asserts, per case:
   - the historical path is a valid path in the compiled graph (every step an
     edge; flags and links honored);
   - deadlines computed by the `deadlines` op for key steps match the rule
     (not the historical filing date);
   - the terminal reached has the right outcome label;
   - where the pack lacks a step the real case took, the test documents the gap
     (expected-failure list) and you file it in NOTES as a pack gap.
   Use expect-test snapshots (the `expect-test` crate is fine if justified) for
   the path rendering, so reviewers see a readable story of each case.
3. Clearer tests overall: add a short README in tests/ explaining the test
   layers; rename cryptic tests you touch to sentence-style names; add a
   `tests/golden/` set of small hand-checkable graphs (2-6 nodes) with hand
   computed expected values for solve/chain/simulate/paths/min-cut, each with a
   comment showing the arithmetic.
4. Fix pack gaps you find only if small and clearly sourced; otherwise list them.

## Ground rules (all litgraph work)
- Read AGENTS.md, CLAUDE.md, docs/ARCHITECTURE.md, docs/CRITIQUE.md,
  docs/COST_FUNCTIONS.md and CONTRIBUTING.md first. Follow them.
- Gates: `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`
  (pedantic), `cargo nextest run --workspace`, line coverage >= 97%
  (`cargo llvm-cov nextest --workspace --summary-only`), no .rs file over 500
  lines, SPDX header + `//!` doc on new files, no unwrap/expect outside tests,
  `typos` clean, no new dependency without justification (`cargo deny check`).
- New ops/fields are discoverable through `litgraph describe` and
  `litgraph schema`; update docs, AGENTS.md if the agent surface changes, add
  an examples/*.json, and an entry under `## [Unreleased]` in CHANGELOG.md.
- Facts about law or real cases come from sources you fetched, recorded with
  URLs. Never invent numbers; label estimates as estimates.
- Commit on this branch with Conventional Commits (a `feat:` for the feature).
  Do not push, merge, or close anything. Write NOTES-<slug>.md at the repo root
  summarizing what you built, gate results, and anything deferred, then delete
  your BRIEF file in the final commit.

