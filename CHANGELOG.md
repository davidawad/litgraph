# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Calibration pipeline** (`docs/CALIBRATION.md`): `calibration/*.json`
  overlay files — embedded at build time like `packs/*.json`, overridable
  via `$LITGRAPH_CALIBRATION`/`--calibration-dir` — record a sourced
  probability, duration, or node attribute (value, source URL/title,
  vintage, sample size `n`, and derivation notes). `scenario.calibration`
  (a list of set names) applies them by mutating the compiled graph exactly
  as if the value had been hand-authored in the pack; `provenance.calibration`
  reports what was applied, from where, and with what `n`. The new
  `calibration` op ranks every *uncalibrated* probability/duration by
  decision sensitivity (the same one-at-a-time perturbation `tornado` uses),
  so an agent knows what to calibrate next. `litgraph schema calibration`
  and `litgraph validate --kind calibration` cover the new file kind. Ships
  with a first, real calibration set: USPTO PTAB institution/FWD rates,
  Federal Circuit affirmance/vacatur rates on CoFC-origin appeals, CoFC
  bid-protest volume, and AO district-court time-to-disposition — each
  sourced from one fetched, working document (see `docs/CALIBRATION.md` for
  citations and what's deliberately left uncalibrated).
- `litgraph-mcp` (`crates/litgraph-mcp`): a stdio MCP server, a thin
  wrapper over `litgraph::api::handle`. Every engine op is an MCP tool,
  each with an input schema sliced directly out of the engine's own
  `schemars` schema for `Op` (so a tool's shape cannot drift from what the
  engine accepts) and a description taken from `describe`'s own `"ops"`
  text; a generic `litgraph` tool takes a raw request verbatim. Packs,
  `links.json`, and the manual are MCP resources
  (`litgraph://packs/<id>`, `litgraph://links`, `litgraph://describe`).
  Honors `LITGRAPH_PACKS`/`--packs-dir` like the CLI. Engine errors
  (`ok: false`) become MCP tool errors carrying the same JSON envelope;
  the server never panics. Ships in the release tarballs, the container
  image, and the nix flake (`packages.litgraph-mcp`) alongside `litgraph`.
  See the README's "MCP server" section for `claude mcp add`, Claude
  Desktop, and generic-client setup.
- `clock` module: dependency-free deadline computation under `FRCP 6`,
  `RCFC 6`, `FRAP 26` (also governs the Federal Circuit), and `19 CFR
  210.6(a)` (ITC Section 337) — federal legal holidays (including
  RCFC-only Inauguration Day), forward/backward day counting,
  calendar-vs-court-day units, the 3-day mail/service rules (differing
  service-method sets per rule set), `FRCP`/`RCFC 6(a)(2)` hours-based
  periods, and `6(a)(3)` clerk-inaccessibility extensions. Ported from
  `civ-pro-the-gathering`'s `src/engine/clock/`, re-verified against the
  rule text.
- `deadlines` API op: concrete due dates, with a step-by-step computation
  trace, for an edge's authored `deadline`, a node's out-edges, or every
  deadline reachable from a node — with `service_method`,
  `additional_holidays`, and `clerk_inaccessible` options. Discoverable via
  `describe`/`schema request`.
- Scenario library (`scenarios/*.json`): named matter profiles (rates,
  stakes, perspective, payoffs, probabilities, policy, which packs to load),
  embedded into the binary like packs and overridable with
  `LITGRAPH_SCENARIOS`/`--scenarios-dir`. A request's `scenario` field can
  now be a bare name (`"scenario": "cofc-1498-patent-case"`) or a named
  scenario composed with inline overrides (`{"extends": "<id>",
  ...overrides}`, deep-merged per RFC 7386, `deny_unknown_fields` preserved
  on the merged result). CLI: `--scenario <name|file|json>`, `--set` composes
  onto a named scenario automatically. `describe` lists every scenario
  (`scenario_library`); `litgraph schema named-scenario` gives the file
  shape; `litgraph validate` accepts a scenario-library file directly
  (`named-scenario` kind). Ships four scenarios: a 28 U.S.C. §1498 patent
  case, a post-award bid protest (28 U.S.C. §1491(b)), an IPR defense at the
  PTAB with appeal to the CAFC, and a generic FRCP civil case; every shipped
  scenario is checked against the current packs by
  `crates/litgraph/tests/scenario_library.rs`. Public loader API:
  `Catalog::scenarios()` / `Catalog::scenario(name)`.

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
