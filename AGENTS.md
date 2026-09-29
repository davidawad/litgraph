# Agent guide

You are driving a litigation procedure engine. Read docs/ARCHITECTURE.md once;
this file is the operating manual.

## Commands

```bash
cargo build --release                      # binary: target/release/litgraph
cargo test                                 # unit + parity (vs the TS engine) + feature tests
litgraph describe                          # the full manual as JSON — start here
litgraph schema request                    # JSON Schema (draft 2020-12) of request|response|scenario|pack|links
litgraph validate my-pack.json             # check a pack/links/scenario/request file; auto-detects kind
litgraph validate - < request.json         # exit 2 if invalid; --kind to force
litgraph packs                             # packs + data quality
litgraph lint [--packs cofc]               # content diagnostics
litgraph q '{"packs":["ptab-patent-trial-appeal-board"],"scenario":{"calibration":["ptab-fy2024"]},"op":{"op":"chain"}}'
                                            # apply real docket-derived rates (docs/CALIBRATION.md)
litgraph q '{"packs":["frcp-civil-procedure"],"op":{"op":"calibration"}}'
                                            # uncalibrated probabilities/durations, ranked by sensitivity
litgraph explain --packs frcp-civil-procedure --arg node=answer-due --set rate=900
litgraph run request.json                  # same as `q`, reads a file
litgraph q '{"packs":["cofc","cafc"],"scenario":{...},"op":{"op":"chain"}}'
litgraph q - < request.json
```

Packs are embedded in the binary at build time — no `packs/` directory is
needed after install. Point at a different set with `LITGRAPH_PACKS=<dir>`
or `--packs-dir <dir>`.

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
   tendencies belong to one matter.
3. **Counterfactuals via `compare`**, not by editing packs.
4. **Pack edits need sources.** Every new `authority`/`cite` must be
   traceable to a `sources` entry or a public citation (`sources[].path` is
   repo-relative only — never a local filesystem path). Run `validate`,
   `lint`, and `cargo test` after any pack change. Never invent rule
   numbers or deadlines; mark uncertainty `UNVERIFIED` in `note`.
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

## Layout

```
crates/litgraph/src/
  model/      packs → compiled graph (schema, links, namespacing, continuations)
  expr/       custom-function language (lexer, parser)
  metrics.rs  built-in metrics/utilities/params (as expressions) + variable envs
  scenario/   Scenario → View (roles, masks, probabilities, node plans, warnings)
  algo/       mdp (solve), chain, sim, paths (dijkstra/yen/pareto), sweep (+tornado), structure
  api/        JSON request/response, describe, op dispatch
  lint.rs     content QA
crates/litgraph-cli/   the `litgraph` binary
packs/                 forum packs + links.json
docs/                  ARCHITECTURE, PACK_SCHEMA, COST_FUNCTIONS, CRITIQUE, CALIBRATION
calibration/           calibration/*.json overlay sets (real, sourced probabilities/durations)
tests/fixtures/ts-parity/   golden outputs from the original TS engine
```

Issue tracking: [GitHub Issues](https://github.com/davidawad/litgraph/issues)
on this repository.
