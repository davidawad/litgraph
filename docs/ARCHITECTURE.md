# litgraph architecture

litgraph models litigation procedure as a **stochastic game on a graph** and
answers the questions a litigator (or an agent working for one) actually has:
*what should we do here, what will it cost, how long, how likely is each
ending, what is the answer most sensitive to, and what changes if X?*

It is designed to be driven by agents first. The test for every design
decision: could an agent, with no prior context, get a correct, sourced,
appropriately-hedged answer in a few cheap calls, and leave the system
better than it found it?

## The tower

Each layer only depends on the layers beneath it. An agent can enter at any
layer and always find the next one up/down by id.

```
 L7  Accretion      lint → fix packs · calibrate probabilities · save scenarios · add links
 L6  Answers        explain · compare · best line   (+ warnings, provenance)
 L5  Analyses       solve · chain · simulate · path · pareto · sweep · tornado · structure
 L4  View           scenario resolved against the graph: roles, active edges,
                    probabilities, costs, utilities, node plans, warnings
 L3  Scenario       the matter: perspective, stakes/payoffs, rates, custom
                    functions, counterfactual masks, calibration overrides, modes
 L2  Graph          packs + links compiled into one namespaced graph
 L1  Packs          one forum's procedure each (FRCP, FRAP, CAFC, CoFC, PTAB, ITC, USPTO…)
 L0  Sources        primary law (rules PDFs, statutes) with sha256, as-of dates
```

Packs are compiled into the `litgraph` binary at build time (`Catalog::embedded`)
so a released binary needs no `packs/` directory alongside it; point at a
different set with `$LITGRAPH_PACKS` or `--packs-dir` (`Catalog::load`).

- **L0 → L1**: every `cite`/`authority` in a pack should trace to a `sources`
  entry (sha256 + as-of). A pack is a claim about the law; sources make it
  checkable.
- **L1 → L2**: `packs/links.json` is where forums meet (district court
  judgment → Federal Circuit; PTAB FWD → CAFC; CoFC judgment → CAFC; CAFC
  remand → back). Ids are namespaced `pack::node`; local ids resolve when
  unique.
- **L2 → L3/L4**: nothing about a particular matter lives in a pack. The
  scenario carries it, and `View` turns it into dense arrays plus a list of
  every fallback taken.
- **L4 → L5**: algorithms consume only the view. They never read JSON, never
  guess, and never special-case a pack.
- **L5 → L6**: the API renders results with ids *and* labels, rounds numbers,
  keeps outputs small by default (`top`, `limit`, opt-in `full_policy`), and
  attaches `warnings` + `provenance` to every response.
- **L6 → L7**: `lint` turns what an analysis exposed (unquantified chance
  nodes, missing payoffs, dead ends, unsourced packs) into concrete edits.

## Design commitments (why an agent can trust and steer it)

1. **One door.** Every capability is `{"op": …}` in one request envelope.
   The CLI, the library, and the MCP server (`litgraph-mcp`) are thin
   wrappers over `api::handle`. `describe` is the complete, machine-readable
   manual:
   ops, scenario fields, built-in metrics with their source expressions,
   variables, functions, parameters and defaults. `litgraph schema
   <request|response|scenario|pack|links>` gives the JSON Schema (draft
   2020-12) for any document kind — machine-checkable before you even run
   anything; `litgraph validate <file|->` (or the `validate` op) resolves a
   pack, `links.json`, scenario, or request without running an analysis,
   reporting parse errors, unresolved references, and lint diagnostics.
2. **No silent guesses.** Every inferred number (heuristic payoff, filled
   probability, deadline-as-duration, ignored interrupt at a mixed node,
   renormalized distribution, dead end) becomes a structured warning with a
   code and location. `provenance.modes` records every modeling choice. An
   agent reading a response can separate authored law from engine defaults
   without reading source.
3. **Counterfactuals are data.** Waivers, missed deadlines, a different
   judge, a settlement offer, a changed rate — all are scenario fields
   (`mask`, `remove_edges`, `probabilities`, `probability_fn`, `payoffs`,
   `policy`, `params`). `compare` runs any op under a base and a
   merge-patched variant and reports deltas. No code, no pack edits.
4. **Custom functions everywhere a number is consumed** (cost, utility,
   mask, probability transform, path weight, Pareto objectives, cut
   capacity, fee eligibility), in one small expression language. Built-ins
   are expressions too, so an agent can read exactly what "dollars" means.
5. **Stable identity.** Packs, nodes and edges have stable ids; errors say
   what was not found and suggest near matches; the same request against the
   same pack fingerprints and engine version returns the same answer
   (simulation is seeded). Parsing is strict everywhere (`deny_unknown_fields`
   on every request, scenario, op, pack, and links document): a typo is a
   hard error naming the valid fields, not a silently-ignored key.
6. **Cheap enough to think with.** Rust, milliseconds per solve on the full
   multi-forum graph, so sweeps, tornados and comparisons fit inside an
   agent's reasoning loop instead of being a batch job. `batch` amortizes
   graph compilation.
7. **Exactness where possible, distributions where necessary.** Values and
   expectations are exact (SCC-ordered Bellman backups, LU for chains);
   simulation is for tails, path-dependent quantities and sampled durations.
8. **Parity before semantics.** The v1 behavior is reproducible via scenario
   modes and pinned by tests, so every semantic improvement is a visible,
   reversible choice.

## Contract and versioning

Every response is one envelope:

```jsonc
{
  "ok": true,
  "api_version": 1,
  "op": "solve",
  "result": { /* op-specific */ },
  "warnings": [ { "code": "payoff-not-authored", "count": 12, "example": "...", "at": [] } ],
  "provenance": { "engine": "litgraph 0.1.0", "packs": [...], "modes": {...} },
  "elapsed_ms": 1.234
}
```

or, on failure, `{"ok": false, "api_version": 1, "op": "...", "error":
{"code", "message", "hint"}, "elapsed_ms": ...}`. `api_version` is the
request/response *contract* version (currently `1`), bumped only when the
shape of the envelope itself changes in a breaking way — separate from
`litgraph`'s own semver release version (`provenance.engine`). `warnings`
is always grouped by code (`{code, count, example, at}`, `at` capped at
eight locations) rather than one entry per occurrence, so a graph with
hundreds of unauthored edges doesn't drown the response.

CLI exit codes: `0` the request ran and `ok: true` (or a `validate`
document is valid); `2` the request ran and failed, or a `validate`
document is invalid; `1` usage or I/O error (bad flags, unreadable file)
before a request was even attempted.

Every document kind in the contract — `Request`, `Response`, `Scenario`,
`Pack`, `LinkFile` — has a generated JSON Schema (draft 2020-12) via
`litgraph schema <kind>`, so a client can validate shapes without
hand-maintaining a parallel schema. `litgraph validate` runs that parse
plus, for packs and requests, the semantic checks (unresolved node/edge
refs, bad expressions) that only show up once the document is resolved
against a compiled graph — the same checks `api::handle` runs, without
running an analysis.

## The agent loop

```
describe ─► packs (quality) ─► explain <node> ─► solve ─► chain / simulate
    │                              │                 │
    │                              └── compare / sweep / tornado (what-ifs, sensitivity)
    └────────────────────────────── lint ─► edit pack / links / scenario ─► re-run
```

1. `describe` once per session (or when an error says so).
2. `packs` to see data quality: payoffs authored? probabilities? hours?
   durations? sources? This bounds how much to trust any number.
3. Build the matter scenario: perspective, `payoffs` for the terminals that
   matter, `rate`, known facts as `mask`/`remove_edges` (a waived defense is
   a removed edge).
4. `explain` at the current node: who decides, each option's value, regret
   vs the best, cost, deadline, and what each option leads to.
5. `solve` / `chain` / `simulate` for the whole-matter picture.
6. `tornado` to find the few inputs that drive the answer; `sweep` to find
   thresholds ("above $X/hour, file the RCE instead"); `compare` for
   discrete alternatives.
7. Report numbers **with** their warnings. Heuristic payoffs and filled
   probabilities are not facts.
8. Accrete: fix what `lint` and the warnings exposed, in the pack, with a
   source.

## Composition model

A composed graph is the union of packs plus link edges. A terminal that has
a link leaving it (e.g. `cofc::judgment-entered` → `cafc::entry-cofc`)
becomes a decision with an explicit zero-cost `accept` edge to an `#end`
twin that keeps its payoff, so "stop here or appeal" is a real choice.
Remand terminals in CAFC link back into the origin forum.

## MCP server

`litgraph-mcp` ([`crates/litgraph-mcp`](../crates/litgraph-mcp)) is a
stdio MCP server: every op is an MCP tool (input schema sliced out of the
engine's own JSON Schema for `Op`, so a tool's shape can't drift from what
`api::handle` actually accepts), plus a generic `litgraph` tool that takes
a raw request verbatim. Packs, `links.json`, and the manual are MCP
resources under `litgraph://…` URIs. See the README's "MCP server"
section for client setup (Claude Code, Claude Desktop, generic clients).

`litgraph://scenarios/<name>` resources are not wired up yet — there is no
scenario library to serve until the roadmap item below lands; the
resource dispatch in `crates/litgraph-mcp/src/resources.rs` is already
shaped so that adding them is one new match arm (tracked as bead lg-3cx).

## Roadmap (tracked as beads)

- WASM build so civ-pro-the-gathering consumes litgraph instead of
  `src/lib/graph` (one engine, two front ends).
- State flags on edges (`sets`/`requires`) compiled to a product graph:
  estoppel, waiver memory, RCE counts, cross-forum preclusion.
- Port civ-pro's deadline clock (court days, holidays, FRCP/RCFC 6).
- Scenario library (`scenarios/*.json`) for named matter profiles.
- ~~Calibration pipeline: probabilities and durations from docket data~~
  **Delivered**: `calibration/*.json` (embedded like packs; `scenario.calibration`,
  `provenance.calibration`, the `calibration` gap-ranking op) — see
  `docs/CALIBRATION.md`. Still open: a CourtListener/PACER-backed fetcher for
  ongoing re-calibration (`scripts/calibrate/` currently regenerates one
  set's arithmetic from recorded constants, not a live docket puller).
- CVaR-optimal policies; general-sum (opponent with its own payoffs).
- Cite verification: every `authority` resolves to a span in an L0 source.
