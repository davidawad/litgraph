# NOTES: settle (lg-ck8)

Settlement prediction from the game the engine already solves: new op
`settle`. Full description in `docs/SETTLEMENT.md`.

## What was built

- `crates/litgraph/src/algo/settle/`
  - `value.rs`: each side's certainty equivalent (walk-away point) under the
    solved general-sum equilibrium policy, under its own payoffs, costs and
    risk attitude (expected / CARA / worst exact; CVaR by seeded Monte Carlo).
    Also the bargaining round length (days to the next timed step) and the
    likely line.
  - `bargain.rs`: ZOPA / no-deal gap; split-the-surplus midpoint (Gould 1973);
    symmetric and weighted Nash (Nash 1950, Kalai 1977; in CARA utility for
    sure money); Rubinstein alternating offers (1982), both proposer orders
    plus the short-round limit (Binmore, Rubinstein & Wolinsky 1986).
  - `stopping.rs`: settling as an always-available action, a one-sided
    optimal-stopping problem per side.
  - `rule68.rs`: FRCP/RCFC 68(d) cost shifting on a plaintiff's judgment
    that is not more favorable than the offer (*Delta v. August* and
    *Marek v. Chesny* scope).
  - `mod.rs`: options, validation, timing (range along the likely line, peak
    surplus, first settle node, policy changes, option value).
- `crates/litgraph/src/api/settle.rs`: op rendering and warnings
  (`settle-zero-sum`, `settle-cvar-sampled`, `settle-cvar-policy`,
  `settle-truncated`, `not-converged`). `settle` shows up in
  `litgraph describe`, `litgraph schema request`, the CLI (`litgraph settle`),
  and MCP (as a tool derived from the schema).
- Docs: `docs/SETTLEMENT.md` (models, citations, Rule 68 sources, worked
  example, limits), ARCHITECTURE, COST_FUNCTIONS, README, AGENTS.md,
  CHANGELOG `[Unreleased]`. Example: `examples/cofc-1498-settle.json`.
- Tests: closed-form two-outcome game (ZOPA, Nash and Rubinstein by hand),
  proptests (price always inside the ZOPA; zero surplus means no deal),
  risk/side/node/Rule 68/CVaR/forced-move/cycle edge cases, and the
  cofc-1498 example.

## Sources checked in this session

- FRCP 68 text re-fetched from <https://www.law.cornell.edu/rules/frcp/rule_68>
  on 2026-09-30. Its sha256 (`559d4970…2b8a`) matches the one recorded in
  `docs/SETTLEMENT.md`. RCFC 68 matches the vendored `sources/rcfc.txt`.
- All six DOIs in the SETTLEMENT.md references were resolved on Crossref.
  Titles, volumes, pages and years match.

## Gates (toolchain 1.97.1, per rust-toolchain.toml)

- `cargo fmt --all --check`: clean
- `cargo clippy --workspace --all-targets -- -D warnings`: clean
- `cargo nextest run --workspace` (via llvm-cov): 523 passed, 1 skipped;
  doctests pass
- `cargo llvm-cov nextest --workspace --summary-only`: 97.76% lines
  (settle files 97.4–100%)
- no `.rs` file over 500 lines; every new file has SPDX + `//!`; no
  unwrap/expect in non-test settle code
- `typos`: clean. This needed a `_typos.toml` allowlist for two short keys
  (`tje`, `tpos`) in `marketing/graph/index.html`, which came from an
  earlier marketing commit.
- `cargo deny check`: advisories, bans, licenses and sources ok. No new
  dependencies (`rand`/`rand_chacha`/`proptest` were already in the
  workspace).

## Caveats and deferred work

- The example's money inputs are illustrative estimates: the 10%/4% rates,
  the $450k offer and the $25k costs, plus the scenario's own payoffs. The
  response also carries `fact-unset`, `mixed-node` and `probability-fill`
  warnings from the scenario.
- On `cofc-1498-patent-case` the judgments are all-or-nothing ($1M/$0), so
  no offer under $1M triggers 68(d). The Rule 68 path is exercised by the
  tests.
- Not modeled: the 14-day timing of the Rule 68 offer, the plaintiff's loss
  of its own post-offer costs, judgment plus pre-offer costs compared with
  the offer, re-solving the policy while an offer is outstanding, a joint
  (two-sided) stopping re-solve, and incomplete information.
