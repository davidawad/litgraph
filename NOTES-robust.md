# NOTES: robust (lg-8vg): Bayesian probabilities, robust solve, value of information

## What was built

Full write-up: `docs/UNCERTAINTY.md`.

1. **Uncertainty model** (`scenario/belief.rs`). Each chance draw gets a
   Dirichlet: chance nodes, nature-first interrupts (with a residual
   "chooser acts" slot), and act-or-wait waits.
   - The prior mean is the resolved probability.
   - The strength comes from `scenario.uncertainty.concentration[node]`,
     else the calibration entry's `n` (new `Graph::sample_size`, filled by
     calibration), else `default_concentration` / 10. The default is
     warned as `prior-concentration-estimated`.
2. **Bayesian updating.** `scenario.observe` (`{node: {edge: count}}`)
   applies the conjugate update in `View::new`, so every op uses the
   posterior mean. Observations are recorded in `provenance.modes.observe`.
3. **Robust solve.** `objective: {type: robust, credibility, radius?,
   samples, seed}` is a rectangular robust MDP (Iyengar 2005; Nilim &
   El Ghaoui 2005).
   - The ambiguity set is a per-node L1 ball around the posterior mean,
     restricted to its support. The radius is the credibility quantile of
     `‖θ − θ̄‖₁` under the Dirichlet posterior (Petrik & Russel 2019, BCI).
   - It is a hook in `mdp::Ctx::aggregate_at`, with the greedy inner
     problem in `algo/robust.rs`.
   - `solve` adds `robust.{nominal_value, robust_value,
     price_of_robustness, ambiguous_nodes, max_radius, policy_changes}`.
4. **Posterior propagation.** `posterior` op (`algo/posterior.rs`): credible
   intervals on the value and on each option's Q, plus P(option optimal).
5. **Value of information.** `voi` op (`algo/voi.rs`). All quantities are
   computed as expected regret against the posterior-mean policy, so they
   are ≥ 0 and exactly 0 when the decision can't change:
   - EVPI of each node's outcome (exact);
   - EVPPI of its probabilities (Monte Carlo);
   - total EVPI;
   - EVSI of `studies` `{node, k, cost | cost_edge, label}`, with `net`
     and `worth_paying`.
6. **Tests.**
   - `tests/uncertainty.rs`: conjugate closed forms (Beta, Dirichlet,
     calibrated `n` = 1087); robust ≤ nominal; radius 0 = nominal;
     radius 2 = `worst`; a robust policy change; the Beta(1, 1) gamble's
     hand-derived EVPI 25 / EVPPI 12.5 / EVSI(1) 25/3; P(optimal) = ½;
     EVPI = 0 when a dominant option exists.
   - `tests/uncertainty_api.rs`: the ops through the contract, errors,
     describe/schema, and a cycle regression.
   - `tests/uncertainty_props.rs` (proptests): conjugate mean, robust ≤
     nominal, EVPI ≥ EVPPI/EVSI ≥ 0, all zero when moot. Also checked at
     `PROPTEST_CASES=1000`.
   - Unit tests for the samplers and the L1 inner problem.

Surface: `describe` (ops, scenario fields, objective), `schema` (automatic),
MCP tools (automatic from the Op schema), and the CLI help list. Also
updated: AGENTS.md (commands, rule 9, layout), README docs list,
ARCHITECTURE, COST_FUNCTIONS (information value is now built-in),
CRITIQUE, and CHANGELOG `[Unreleased]`. New examples:
`examples/ptab-robust-institution.json`,
`cofc-posterior-observed-judge.json`, `cofc-voi-mock-sj-panel.json`. The
counts and prices in them are illustrative inputs and are labeled so in the
docs.

No new dependencies. Gamma sampling is Marsaglia–Tsang and the normal is
Box–Muller, on the existing `rand`/`rand_chacha`.

## Gate results (pinned toolchain 1.97.1)

The sandbox only had stable 1.98.1 system-wide, so 1.97.1 was installed
into a private `RUSTUP_HOME`.

- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --all-targets -- -D warnings` (pedantic): clean.
  On stable 1.98.1, newer lints fire on existing code (`manual_midpoint`
  in `sweep.rs`, `unused_async` in `litgraph-mcp/src/server.rs`). These
  were left untouched.
- `cargo nextest run --workspace`: 519 passed, 1 skipped. `cargo test
  --doc`: ok.
- `cargo llvm-cov nextest -p litgraph --fail-under-lines 97`: 98.05% lines.
- No `.rs` file over 500 lines. SPDX header and `//!` doc on every new file.
  No unwrap/expect outside tests.
- `cargo deny check`: advisories, bans, licenses, sources ok.
- `typos`: clean for everything this branch touches. The existing hits in
  `marketing/graph/index.html` (two short identifiers) were not touched.
- `cargo audit`: not run (not installed in the box).

## Design notes and deferred work

- **Outcome EVPI is not reported for nodes on a cycle** (`null`). Forcing
  one outcome on every visit loops forever. The first run on the CoFC
  scenario gave a nonsense 2.7e8 at `ruling-12b` before this guard;
  there is now a regression test. These nodes always get the Monte Carlo
  EVPPI instead.
- `posterior` and `voi` are risk-neutral and zero-sum. Any other objective
  or `opponent_objective` is warned (`uncertainty-risk-neutral`).
  Information is modeled as public.
- Robust credibility is per node, not joint (a union bound; documented).
  The KL ambiguity set is not implemented, since L1 has the exact greedy
  inner step. Robust plus `opponent_objective` is warned and not combined.
- **Deferred:**
  - learning *within* a trajectory (a POMDP);
  - exact (enumerated) EVSI for small `k`;
  - a joint credible bound across nodes;
  - lowering the default prior strength for population-level calibration
    `n` automatically (currently the user sets `concentration` by hand).

## Bead status (local scratch)

lg-8vg: implemented and committed on this branch. Left OPEN; not closed
here, per instructions.
