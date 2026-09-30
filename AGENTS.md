# Agent guide

You are driving a litigation procedure engine. Read docs/ARCHITECTURE.md once;
this file is the operating manual.

## Commands

```bash
cargo build --release                      # binary: target/release/litgraph
cargo test                                 # unit + parity (vs the TS engine) + feature tests
litgraph describe                          # the full manual as JSON — start here (lists scenario_library)
litgraph schema request                    # JSON Schema (draft 2020-12) of request|response|scenario|named-scenario|pack|links
litgraph validate my-pack.json             # check a pack/links/scenario/named-scenario/request file; auto-detects kind
litgraph validate - < request.json         # exit 2 if invalid; --kind to force
litgraph packs                             # packs + data quality
litgraph lint [--packs cofc]               # content diagnostics
litgraph q '{"packs":["ptab-patent-trial-appeal-board"],"scenario":{"calibration":["ptab-fy2024"]},"op":{"op":"chain"}}'
                                            # apply real docket-derived rates (docs/CALIBRATION.md)
litgraph q '{"packs":["frcp-civil-procedure"],"op":{"op":"calibration"}}'
                                            # uncalibrated probabilities/durations, ranked by sensitivity
litgraph q '{"scenario":{"extends":"cofc-1498-patent-case","observe":{"cofc::sj-ruling":{"cofc::e-sj-govt":3}}},"op":{"op":"solve"}}'
                                            # Bayesian update on observed outcomes (docs/UNCERTAINTY.md)
litgraph q '{"scenario":{"extends":"cofc-1498-patent-case","objective":{"type":"robust"}},"op":{"op":"solve"}}'
                                            # robust to the credible range of probabilities; nominal vs robust + policy changes
litgraph q '{"scenario":"cofc-1498-patent-case","op":{"op":"posterior"}}'   # credible intervals, P(option optimal)
litgraph q '{"scenario":"cofc-1498-patent-case","op":{"op":"voi","studies":[{"node":"cofc::sj-ruling","k":5,"cost":25000}]}}'
                                            # what is worth paying to learn (EVPI / EVSI vs cost)
litgraph explain --packs frcp-civil-procedure --arg node=answer-due --set rate=900
litgraph run request.json                  # same as `q`, reads a file
litgraph q '{"packs":["cofc","cafc"],"scenario":{...},"op":{"op":"chain"}}'
litgraph q '{"scenario":"cofc-1498-patent-case","op":{"op":"chain"}}'   # named scenario; its own `packs` apply
litgraph chain --scenario cofc-1498-patent-case --set rate=900          # CLI: name/file/inline JSON, --set composes via `extends`
litgraph q - < request.json
litgraph q - < examples/cofc-1498-settle.json   # settle: ZOPA, Nash/Rubinstein price, when to settle, Rule 68
                                            # (needs scenario.opponent_objective; docs/SETTLEMENT.md)
```

Packs are embedded in the binary at build time — no `packs/` directory is
needed after install. Point at a different set with `LITGRAPH_PACKS=<dir>`
or `--packs-dir <dir>`. The named scenario library (`scenarios/*.json`) is
embedded the same way; override with `LITGRAPH_SCENARIOS=<dir>` or
`--scenarios-dir <dir>`. See docs/PACK_SCHEMA.md's "Scenario library" section
for the file shape and how a request composes a named scenario with
overrides (`{"extends": "<id>", ...}`). The vendored L0 source corpus
(`sources/*.txt`, see `sources/PROVENANCE.md`) is embedded the same way;
override it with `LITGRAPH_SOURCES=<dir>`.

Driving this from an MCP-capable agent instead of the CLI: `litgraph-mcp`
(`crates/litgraph-mcp`) is the same contract over stdio — every op above is
its own MCP tool, plus a generic `litgraph` tool for a raw request; packs,
`links.json`, the named scenario library, and this manual are MCP resources
(`litgraph://packs/<id>`, `litgraph://links`, `litgraph://scenarios`,
`litgraph://scenarios/<id>`, `litgraph://describe`). `claude mcp add
litgraph -- litgraph-mcp` wires it into Claude Code; see the README's "MCP
server" section for other clients.

Every response: `{ok, api_version, op, result, warnings, provenance,
elapsed_ms}` or `{ok:false, api_version, op, error:{code, message, hint},
elapsed_ms}`. `warnings` is grouped: `[{code, count, example, at}]`, `at`
holding up to eight locations. `api_version` is the request/response
contract version (bumped only on a breaking change; currently `1`).
Exit codes: `0` ok, `2` the request ran and failed / a document is invalid,
`1` usage or I/O error.

Parsing is strict everywhere — packs, scenarios, requests, and op
arguments reject unknown fields with an error naming the valid ones,
rather than silently ignoring a typo.

## Rules

1. **Report warnings with numbers.** `payoff-not-authored`,
   `probability-fill`, `mixed-node`, `sink` mean the number rests on a
   fallback. Say so, or fix the input (scenario `payoffs`/`probabilities`)
   before answering.
2. **Matter facts go in the scenario, not the pack.** Packs describe a
   forum's procedure for everyone; stakes, rates, waivers, the judge's
   tendencies belong to one matter. A chance node whose outcome is actually
   knowable at filing (time-barred? already pending elsewhere? a patent
   case?) is tagged `"fact"` in the pack with an authored prior; the
   scenario resolves it for real via `scenario.facts` (`{node ref: edge
   ref}`), not by editing the pack. See `docs/PACK_SCHEMA.md#matter-facts-v2`.
3. **Counterfactuals via `compare`**, not by editing packs.
4. **Pack edits need sources.** Every new `authority`/`cite` must be
   traceable to a `sources` entry or a public citation (`sources[].path` is
   repo-relative only — never a local filesystem path). Run `validate`,
   `lint`, and `cargo test` after any pack change. Never invent rule
   numbers or deadlines; mark uncertainty `UNVERIFIED` in `note`. If
   `sources[].path` points at a file under `sources/`, `lint` verifies the
   cite mechanically (`unverifiable-cite` warning with the exact reason if
   it can't) — see `docs/PACK_SCHEMA.md`'s authoring rule 1.
5. **Probabilities**: only on non-self edges, with basis + vintage in
   `note`. Unauthored beats invented.
6. **Pin modes when comparing to v1** (`mixed: optimistic`,
   `prob_fill: uniform`); see tests/parity.rs.
7. **Composed graphs**: set `scenario.start` explicitly; the default start
   is the first pack's start (alphabetical when `packs` is empty).
8. **Calibrate, don't guess.** Before hand-typing a probability/duration
   teaching estimate, check whether `calibration/*.json` already has a
   sourced value, or whether `{"op":"calibration"}` says this input is worth
   calibrating next. See `docs/CALIBRATION.md`.
9. **Soft probabilities are uncertain, say how much.** Matter-specific
   evidence ("this judge granted 3 of 4") goes in `scenario.observe`, not in
   `probabilities`. Before recommending a line that rests on soft
   probabilities, check `posterior` (P(option optimal), credible interval)
   or `objective: robust`'s `policy_changes`; before recommending paying for
   information, check `voi`'s EVSI against its cost. Report
   `prior-concentration-estimated` warnings: the prior strength there is a
   default estimate, not a sample size. See `docs/UNCERTAINTY.md`.

## Layout

```
crates/litgraph/src/
  model/      packs → compiled graph (schema, links, namespacing, continuations)
  expr/       custom-function language (lexer, parser)
  metrics.rs  built-in metrics/utilities/params (as expressions) + variable envs
  scenario/   Scenario → View (roles, masks, probabilities, node plans, belief, warnings)
  algo/       mdp (solve), chain, sim, paths (dijkstra/yen/pareto), sweep (+tornado), structure,
              robust, posterior, voi, dirichlet (uncertain probabilities)
  scenario/   Scenario → View (roles, masks, probabilities, node plans, warnings)
  algo/       mdp (solve), chain, sim, paths (dijkstra/yen/pareto), sweep (+tornado), structure,
              equilibrium (general-sum), cvar, settle (ZOPA, bargaining prices, stopping, Rule 68)
  api/        JSON request/response, describe, op dispatch
  lint.rs     content QA
  cite/       cite verification against L0 sources (normalize, corpus, fuzzy match)
crates/litgraph-cli/   the `litgraph` binary
crates/litgraph-mcp/   the `litgraph-mcp` binary: stdio MCP server, one tool per op
packs/                 forum packs + links.json
sources/               vendored L0 primary-law text (see sources/PROVENANCE.md)
calibration/           calibration/*.json overlay sets (real, sourced probabilities/durations)
docs/                  ARCHITECTURE, PACK_SCHEMA, COST_FUNCTIONS, CRITIQUE, CALIBRATION, UNCERTAINTY
docs/                  ARCHITECTURE, PACK_SCHEMA, COST_FUNCTIONS, CRITIQUE, CALIBRATION, SETTLEMENT
tests/fixtures/ts-parity/   golden outputs from the original TS engine
tests/cases/           famous cases replayed through the packs (+ story snapshots)
tests/golden/          hand-checked 2-6 node graphs; tests/README.md explains the layers
```

Issue tracking: [GitHub Issues](https://github.com/davidawad/litgraph/issues)
on this repository.
