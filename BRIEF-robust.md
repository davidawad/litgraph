# Brief: uncertainty that changes decisions (Bayesian, robust, value of information)

Goal: treat probabilities as uncertain, choose strategies that hold up across
that uncertainty, and price information.

1. Probability uncertainty model: per edge (or per chance node) an optional
   Beta/Dirichlet with pseudo-counts. Sources: calibration entries' sample size
   `n` (calibration/*.json already carries n), otherwise a default
   concentration labeled as an estimate. Scenario may override.
2. Bayesian updating: an `observe` input (observed outcomes at chance nodes,
   e.g. this judge granted 3 of 4 similar motions) updates the Dirichlet
   posteriors; all ops then use the posterior mean.
3. Robust solve: an objective that maximizes the worst-case value over a
   credible set (per-node Dirichlet credible region; implement the standard
   rectangular robust MDP backup with an L1 or KL ambiguity set; cite the
   method). Report nominal vs robust value and where the policy changes.
4. Posterior propagation: Monte Carlo over the posterior to give a credible
   interval on the case value and on each action's Q, and the probability each
   action is optimal.
5. Value of information: EVPI (perfect information at a chance node or
   parameter) and EVSI for a sample (e.g. an expert report equivalent to k
   observations) = expected improvement in decision value; rank what is worth
   paying to learn. Compare to the step's cost.
6. Tests: conjugate-update closed forms, robust value <= nominal, EVPI >= EVSI
   >= 0, EVPI == 0 when the decision can't change, plus proptests.

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

