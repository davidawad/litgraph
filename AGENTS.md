# Agent guide

You are driving a litigation procedure engine. Read docs/ARCHITECTURE.md once;
this file is the operating manual.

## Commands

```bash
cargo build --release                      # binary: target/release/litgraph
cargo test                                 # unit + parity (vs the TS engine) + feature tests
litgraph describe                          # the full manual as JSON — start here
litgraph packs                             # packs + data quality
litgraph lint [--packs cofc]               # content diagnostics
litgraph explain --packs frcp-civil-procedure --arg node=answer-due --set rate=900
litgraph q '{"packs":["cofc","cafc"],"scenario":{...},"op":{"op":"chain"}}'
litgraph q - < request.json
```

Every response: `{ok, op, result, warnings, provenance, elapsed_ms}` or
`{ok:false, error:{code, message, hint}}` (exit code 2).

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
   traceable to a `sources` entry or a public citation. Run `lint` and
   `cargo test` after any pack change. Never invent rule numbers or
   deadlines; mark uncertainty `UNVERIFIED` in `note`.
5. **Probabilities**: only on non-self edges, with basis + vintage in
   `note`. Unauthored beats invented.
6. **Pin modes when comparing to v1** (`mixed: optimistic`,
   `prob_fill: uniform`); see tests/parity.rs.
7. **Composed graphs**: set `scenario.start` explicitly; the default start
   is the first pack's start (alphabetical when `packs` is empty).

## Layout

```
crates/litgraph/src/
  model.rs      packs → compiled graph (namespacing, links, continuations)
  expr.rs       custom-function language
  metrics.rs    built-in metrics/utilities/params (as expressions) + variable envs
  scenario.rs   Scenario → View (roles, masks, probabilities, node plans, warnings)
  algo/         mdp (solve), chain, sim, paths (dijkstra/yen/pareto), sweep (+tornado), structure
  api.rs        JSON request/response, describe
  lint.rs       content QA
crates/litgraph-cli/   the `litgraph` binary
packs/                 forum packs + links.json
docs/                  ARCHITECTURE, PACK_SCHEMA, COST_FUNCTIONS, CRITIQUE
tests/fixtures/ts-parity/   golden outputs from the original TS engine
```

Issue tracking: `br` (beads_rust) in this repo — `br ready`, `br show <id>`.
