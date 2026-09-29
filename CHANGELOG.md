# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-09-28

Initial public release.

litgraph models litigation procedure as a stochastic game on a graph and
answers the questions a litigator (or an agent) actually has: what to do,
what it will cost, how likely each ending is, what the answer is most
sensitive to, and what changes under any counterfactual.

Extracted and rewritten in Rust from the TypeScript engine in
[civ-pro-the-gathering](https://github.com/davidawad/civ-pro-the-gathering),
and verified against the original engine on all six original packs
(`crates/litgraph/tests/parity.rs`).

#### Engine

- One request envelope (`{op, scenario, ...}`) → one response envelope
  (`{ok, api_version, op, result, warnings, provenance, elapsed_ms}` or
  `{ok:false, api_version, op, error}`), served identically by the CLI and
  the library crate. `warnings` is grouped by code
  (`{code, count, example, at}`); `api_version` is the request/response
  contract version, independent of the crate's own semver release.
- Ops: `describe`, `packs`, `lint`, `validate`, `graph`, `metric`,
  `explain`, `solve`, `chain`, `simulate`, `path`, `pareto`, `sweep`,
  `tornado`, `structure`, `compare`, `batch`.
- CLI: `litgraph schema <request|response|scenario|pack|links>` (JSON
  Schema, draft 2020-12); `litgraph validate <file|->` (parses + resolves
  a pack/links/scenario/request, auto-detecting kind, exit 2 if invalid);
  `litgraph run <request.json>`; plus `q`, `describe`, and shorthand
  `<op> --packs --set --arg --scenario`. Packs are embedded in the binary
  at build time (`LITGRAPH_PACKS` or `--packs-dir` to override), so no
  `packs/` directory is needed after install.
- Parsing is strict everywhere (`deny_unknown_fields`): a typo in a
  request, scenario, op, pack, or links document is an error listing the
  valid fields, not a silently-ignored key.
- Custom cost/utility/probability/weight functions via a small expression
  language, evaluated everywhere a number is consumed (cost, utility, mask,
  probability transform, path weight, Pareto objectives, cut capacity, fee
  eligibility). Built-in metrics/utilities are themselves named expressions,
  visible via `describe`.
- Every inferred number (heuristic payoff, filled probability,
  deadline-as-duration, dead end, non-convergence) is reported as a
  structured warning with a code and location; `provenance.modes` records
  every modeling choice made.
- Packs compose into one namespaced graph via `packs/links.json`; pack
  **instances** (e.g. `cafc@cofc`) let a shared forum (the Federal Circuit)
  remember how it was entered — origin-specific remand routing, perspective
  flips, payoff transforms, edge patches.
- Solver: SCC-ordered Bellman backups (exact where the graph is acyclic,
  iterative within cycles), LU-solved absorbing chains, Dijkstra/Yen path
  search, Pareto frontiers, min-cut/dominators/betweenness structural
  analysis, seeded Monte Carlo simulation with CVaR/percentiles.
- Scenario fields make every v1-inherited modeling choice explicit and
  switchable: `mixed` node semantics, `prob_fill`, `opponent` model,
  `objective` (expected / CARA / worst-case), `perspective`, `fee_shift`,
  `discount_annual`.
- `lint` turns analysis findings (unquantified chance nodes, missing
  payoffs, dead ends, unsourced packs) into concrete pack-editing guidance.

#### Content packs (`packs/`)

- Eight forum packs: FRCP civil procedure, FRAP appellate procedure,
  Federal Circuit (CAFC), Court of Federal Claims (CoFC), PTAB, ITC § 337,
  USPTO prosecution (MPEP), FRCrimP criminal procedure.
- CoFC and CAFC packs are schema v2: authored terminal `payoff`/`outcome`
  tags, `sources` with official URL + sha256 + as-of date, `roles`, `attrs`.
  The other six packs are schema v1 (ported unchanged from
  civ-pro-the-gathering), with the engine's label-heuristic payoff fallback
  flagged on every response.
- `tests/fixtures/ts-parity/` — golden output from the original TypeScript
  engine, one file per pack, used by `cargo test` to verify the Rust port
  reproduces v1 behavior exactly (value iteration, absorbing chains,
  Dijkstra, SCCs, dominators, min-cut, Pareto frontiers).

#### Known limits (see `docs/CRITIQUE.md`)

- Markov on the node: litigation history (estoppel, waivers, RCE counts)
  isn't yet expressible as edge preconditions (planned: state-flag product
  graph).
- Payoffs are teaching estimates or placeholders outside the CoFC/CAFC
  packs; always set `scenario.payoffs` for the matter at hand.
- Mean-optimal by default; CVaR/percentiles are reported from simulation
  but not yet optimized.
- Court-day/holiday deadline computation (FRCP/RCFC Rule 6) is not yet
  ported from civ-pro-the-gathering's clock module.
