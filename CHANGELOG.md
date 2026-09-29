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
  `links.json`, the named scenario library, and the manual are MCP
  resources (`litgraph://packs/<id>`, `litgraph://links`,
  `litgraph://scenarios` — an index of every named scenario —,
  `litgraph://scenarios/<id>`, `litgraph://describe`). Honors
  `LITGRAPH_PACKS`/`--packs-dir` and `LITGRAPH_SCENARIOS`/`--scenarios-dir`
  like the CLI. Engine errors
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
- **Cite verification against L0 sources** (`crates/litgraph/src/cite/`):
  `litgraph lint` now checks every node `cite` / edge `authority` string
  against the pack's own `sources` — normalizing surface forms ("FRCP
  12(b)(6)", "Fed. R. Civ. P. 12(b)(6)", "28 U.S.C. § 1498(a)", "35 USC
  315(e)", "37 C.F.R. § 42.108", "RCFC 56", compound citations like
  "28 U.S.C. 1291; 2106", case citations recognized and skipped as
  out-of-scope), resolving each to a vendored source span via
  `Pack.sources[].path`, and fuzzy-matching a quoted phrase in `note`
  against that span when one is given. Unverifiable cites are a new `warn`
  diagnostic, `unverifiable-cite`, with the exact reason. Vendored L0
  corpus under `sources/*.txt` (FRCP, FRAP, 19 U.S.C. § 1337, 37 C.F.R.
  Part 42, and 28/35 U.S.C. + 37 C.F.R. Part 1 fragments — see
  `sources/PROVENANCE.md` for why plain text was vendored instead of
  fetched at lint/test time, and each file's exact provenance/sha256),
  embedded into the binary the same way `packs/` is, with a
  `LITGRAPH_SOURCES` directory override mirroring `LITGRAPH_PACKS`. No
  pack currently wires `sources[].path` to vendored text yet (tracked as a
  follow-up); `cargo test --test cite_coverage_report -- --nocapture`
  reports current coverage across every embedded pack.
- **Matter facts** (`scenario.facts`): a chance node tagged `"fact"` in a pack
  represents a matter fact knowable at filing (time-barred? already pending
  elsewhere? a patent case?), not real uncertainty. `scenario.facts` (`{node
  ref: edge ref}`, the same shape as `policy`) forces that node's true branch;
  left unset, the engine falls back to the pack's authored prior and warns
  `fact-unset`. New lint rule `fact-no-prior` flags a `fact`-tagged node whose
  out-edges aren't all authored. Applied to `cofc::limitations-check` and
  `cofc::section-1500-check`. See `docs/PACK_SCHEMA.md#matter-facts-v2`.
- **District-court patent route**: `packs/links.json` gains a `cafc@district-court`
  instance (mirroring `cafc@cofc`/`cafc@ptab`/`cafc@itc`) and a link from
  `frcp-civil-procedure::notice-of-appeal-filed` to it, alongside the existing
  regional-circuit (FRAP) link, for 28 U.S.C. § 1295(a)(1) patent appeals.
  Which applies is a matter fact, not a free choice between forums — a
  scenario selects the applicable route with `scenario.remove_edges`.
- `frap-appellate-procedure`, `frcp-civil-procedure`, `frcrimp-criminal-procedure`,
  `itc-337`, `mpep-prosecution`, and `ptab-patent-trial-appeal-board` migrated
  to schema v2: authored terminal `payoff`/`outcome`, `roles`, `sources`
  (verified official URLs, several with `sha256`), stable `id`s on every
  parallel edge, and `duration` on every deadline-bearing edge plus several
  major no-deadline steps (sourced from the underlying rule's own timeline
  where one exists, otherwise a labeled estimate). `frap`/`frcrimp`/`ptab`
  also gain authored `hours` on every choice edge. `litgraph lint` reports no
  payoff/duration-authoring/role/source/parallel-id gaps on any of the six.
  `tests/fixtures/ts-parity/v1-packs/` freezes the pre-migration v1 pack
  content the TS-parity golden output was generated from, so
  `crates/litgraph/tests/parity.rs` keeps checking the engine against it
  independent of `packs/`'s own evolution.
- **State flags** on edges (`sets`/`clears`/`requires`/`forbids`, pack
  schema v2, additive): compiled into a product graph over `(node,
  flag-set)` so a node's identity can depend on procedural history — an IPR
  estoppel, a waived defense, a prior RCE — without any algorithm (`solve`,
  `chain`, `simulate`, `path`, `pareto`, `sweep`, `structure`) needing to
  change. Only reachable `(node, flag-set)` combinations are materialized; a
  pack that never declares a flag compiles to an identical graph. A hard cap
  (`CompileOptions.max_product_nodes` / request `max_product_nodes`, default
  20,000) fails compilation with a clear error instead of an unbounded
  compile. Flagged node ids are stable and readable
  (`ptab::fwd-issued{ipr-estopped}`); a compiled `Node` carries `flags` and
  `base_id` so API responses can project back to the base pack node.
  Terminals can carry `payoffByFlag` to vary payoff by which flag matched.
  `flag("x")` is available in edge and terminal expressions. New
  `CompileOptions.no_flags` / request `no_flags` reproduces the pre-flags
  compile exactly (used by `tests/parity.rs`). See
  `docs/PACK_SCHEMA.md#state-flags`.
- Modeled with real, sourced state-flag examples: IPR estoppel after a final
  written decision (35 U.S.C. § 315(e), `ptab-patent-trial-appeal-board.json`),
  a waived personal-jurisdiction/venue/process defense (FRCP 12(h)(1),
  `frcp-civil-procedure.json`), an RCE already filed (37 C.F.R. § 1.114,
  `mpep-prosecution.json`), and a fix for `cafc-federal-circuit.json`'s
  `cert-not-sought`/`cert-denied` terminals losing the panel's actual
  win/loss through a rehearing-or-cert detour (docs/CRITIQUE.md #13).
- `objective: {type: cvar, alpha, grid?, y_lo?, y_hi?}` optimizes `CVaR_alpha`
  of the total outcome (not just reports it, as `simulate` already did) via
  Rockafellar–Uryasev: a grid-discretized augmented-state backward
  induction, with the outer VaR-threshold search a lookup on the same grid.
  `y_lo`/`y_hi` (set together) override the default grid-bound heuristic for
  a graph where it's a poor fit or unnecessarily wide. Verified against
  brute-force enumeration of deterministic policies on small graphs
  (`tests/cvar.rs`/`tests/cvar_edge_cases.rs`, including a proptest).
  Documented exactness limits (grid discretization; doesn't compose with
  `discount_annual`, `fee_shift`, or a general-sum opponent) in
  `docs/CRITIQUE.md`.
- `scenario.opponent_objective` (a terminal expression): the opponent
  maximizes their own payoff (general-sum) instead of minimizing ours
  (zero-sum), solved as a subgame-perfect equilibrium by backward induction
  on the graph's SCC DAG (cyclic components iterate to a fixed point, with
  the same honest non-convergence reporting as `solve`). `self`'s
  `Objective::Cara`/`Worst` risk objective is honored (applied to self's
  aggregation over nature's draws only, via the same `mdp::aggregate` helper
  `mdp::solve` uses — never to the opponent's, which stays plain
  expectation). Reports both players' values (`solve`'s `opponent_value`/
  `opponent_values`) and gives `explain`'s per-option `regret` the mover's
  own criterion (the opponent's own `opp_q` at their node, not an assumed
  adversary). `opponent_objective: None` (default) reproduces the existing
  zero-sum answer exactly (`tests/general_sum.rs`). Documented in
  `docs/CRITIQUE.md`.
- Path-dependent terminal variables (`spent`, `elapsed_total`, `steps`) in
  utility (and `fee_shift.eligible`) expressions, for prejudgment interest
  and time-growing damages (e.g. `payoff * (1+r)^(elapsed_total/365)`).
  Exact in `simulate` (evaluated per sampled trajectory); `solve`/`chain`
  are Markov on the node and see them as `0`, with a `path-variable-in-markov`
  warning from `validate`/every scenario-resolving op when an expression
  depends on one. See `docs/COST_FUNCTIONS.md` and
  `examples/cofc-prejudgment-interest-sim.json`.
- `scenario.facts` now works on **mixed** (choice+chance) nodes, not just
  pure chance nodes: `litgraph lint`'s `fact-no-prior` check and the
  response's `fact-unset` warning both now look at a fact node's non-applicant
  interrupt edges specifically (`scenario/plan.rs`'s `chooser()`/`act_or_wait()`
  path), so a `fact`-tagged mixed node gets the same diagnostics a pure
  chance one always did, instead of the generic `mixed-node` hedge.
  `ptab-patent-trial-appeal-board.json`'s `petition-threshold-review`
  (35 U.S.C. §315(b) time-bar) is now tagged `fact` as the worked example.
  New `Graph::node_family`/`Graph::edge_family` (generalized from the
  calibration fix above) ensure a fact forced on a base node also forces
  every [state-flag](docs/PACK_SCHEMA.md#state-flags) product-graph copy of
  it — demonstrated on a real flagged copy,
  `petition-threshold-review{prior-petition-denied}` (General Plastic's
  follow-on-petition doctrine, 35 U.S.C. §314(a), on
  `institution-denied-merits`/`-fintiv`'s refile edges).
- New canary test (`every_embedded_pack_file_parses_individually_against_the_strict_pack_struct`)
  parses every embedded pack file individually against the strict `Pack`
  struct, so a structurally-invalid pack (e.g. a duplicate top-level JSON
  key introduced by an unlucky line-based merge -- syntactically valid to a
  generic parser, but rejected by serde's derived `Deserialize`) fails as
  one clearly-named test instead of a whole-suite cascade of near-identical
  panics.
- **A malformed pack or `links.json` no longer takes the whole catalog
  down.** `Catalog::from_files` (embedded set, `LITGRAPH_PACKS`/`--packs-dir`
  directories, `Catalog::load`) used to fail the entire catalog -- every
  pack, every op -- on the first file that didn't parse. It now skips that
  file and records it in the new `Catalog.load_errors` (`{file, message}`),
  keeping every other pack usable; a request naming the broken pack gets an
  ordinary `NotFound` (it was simply never loaded), not a global outage.
  `litgraph describe`'s new `pack_load_errors` names what failed and why, so
  the failure stays visible instead of silent. `Pack::from_json`'s own
  per-file strictness (schema, `deny_unknown_fields`) is unchanged -- this
  is about the blast radius of one bad file, not about being lenient with
  its content.

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
