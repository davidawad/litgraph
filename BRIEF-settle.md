# Brief: settlement prediction from both sides' values

Goal: answer "what should this case settle for, and when?" from the game the
engine already solves.

1. New op `settle`. Inputs: a scenario with an opponent objective (the
   general-sum `opponent_objective` already exists), optional per-side discount
   rates / patience, per-side risk attitude (reuse CARA/CVaR objectives), and
   optionally a node to evaluate from (default start).
2. From any node, compute each side's continuation value (certainty equivalent
   under its own objective) = its walk-away point. Output the bargaining range
   (ZOPA) when plaintiff's reservation <= defendant's, or report "no deal zone"
   with the gap.
3. Predict a price inside the range with standard models, reported side by side:
   Nash bargaining solution (symmetric and with bargaining-power weight),
   Rubinstein alternating offers (discount factors from per-side patience and
   the durations/costs of the next procedural step), and the split-the-surplus
   midpoint. Cite the models in docs.
4. Timing: along the solved best line, compute the range at each node and
   report where settlement surplus peaks (e.g. after a dispositive-motion
   ruling), i.e. "when" to settle. Treat settling as an always-available action
   and show how the policy changes (optimal stopping).
5. Rule 68 offer of judgment: optional offer amount; model FRCP 68(d) cost
   shifting when the final judgment is not more favorable than the offer.
   Verify the rule text on law.cornell.edu.
6. Tests: closed-form checks (a two-outcome game where ZOPA and Nash/Rubinstein
   prices are hand-computable), property tests (price always inside ZOPA; zero
   surplus -> no deal), plus an example on cofc-1498-patent-case.

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

