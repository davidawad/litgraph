# litgraph

**A litigation-procedure graph engine.** litgraph models a court/agency
forum's procedure (FRCP, FRAP, Federal Circuit, Court of Federal Claims,
PTAB, ITC § 337, USPTO prosecution, FRCrimP) as a directed graph, composes
any set of forums into one graph, and turns that into a **stochastic game**:
what should the protagonist do at each step, what will it cost, how long
will it take, how likely is each ending, what does the answer depend on
most, and what changes under any counterfactual (a waived defense, a
different judge, a changed rate, a settlement offer)?

Every number an algorithm consumes — cost, utility, probability transform,
path weight, Pareto objective, fee-shift eligibility — is a small,
inspectable **expression**, evaluated per edge or terminal. Built-in metrics
(`dollars`, `elapsed`, `surprise`, `ev`, ...) are just named expressions, so
you can read exactly what "cost" means and write your own.

> **Not legal advice.** litgraph's probabilities are teaching estimates
> (vintage noted in the pack, where authored at all) and most payoffs are
> placeholders, not real damages figures. Every response reports which
> numbers are authored law/data vs. engine fallbacks (`warnings`,
> `provenance.modes`) — read them before trusting a number. This project
> does not represent anyone, does not predict real case outcomes, and is
> not a substitute for counsel.

## Who it's for

- **Litigators and law students** who want to reason quantitatively about
  procedure: "is this appeal worth briefing at this stake?", "what does the
  § 1500 trap actually cost us?", "which of these three deadlines is the
  one to protect first?"
- **Agents** (LLM-driven tools) that need a machine-checkable model of court
  procedure to answer a specific matter's questions, with every assumption
  reported back instead of hidden in prose.
- **Builders of litigation-adjacent tools** who want a solved, sourced graph
  of a forum's procedure as a library, not a document to re-parse.

## What it does

| capability | op |
|---|---|
| best move at a node, with regret vs. every alternative | `explain` |
| value + best line for the whole matter | `solve` |
| absorption probabilities + expected cost/time under the optimal policy | `chain` |
| outcome distribution, CVaR, P(loss) via Monte Carlo | `simulate` |
| shortest / k-shortest lines to a node, a set of terminals, or an outcome tag | `path` |
| cost × time × likelihood frontier | `pareto` |
| value curve + policy breakpoints as any parameter varies | `sweep` |
| which few inputs actually drive the answer | `tornado` |
| SCCs, dominators, min-cut, betweenness, reachability | `structure` |
| base scenario vs. a counterfactual, with deltas | `compare` |
| several ops against one compiled graph | `batch` |
| content QA: unsourced packs, missing payoffs, dead ends | `lint` |
| resolve packs + scenario without running an analysis | `validate` |

Every response is `{ok, api_version, op, result, warnings, provenance,
elapsed_ms}` or `{ok:false, api_version, op, error:{code, message, hint},
elapsed_ms}`. `warnings` is grouped by code (`{code, count, example, at}`)
rather than one entry per occurrence. `litgraph describe` is the full
machine-readable manual: every op, every scenario field, every built-in
metric with its source expression, every variable and function. Every
document kind (`request`, `response`, `scenario`, `pack`, `links`) has a
JSON Schema via `litgraph schema <kind>`, and `litgraph validate <file|->`
checks any of them — including a pack you're authoring — without running
an analysis.

## Quickstart

```bash
cargo build --release
./target/release/litgraph describe                 # the manual
./target/release/litgraph packs                     # what's loaded + data quality
./target/release/litgraph explain --packs cofc --arg node=complaint-filed
./target/release/litgraph q '{
  "packs": ["cofc", "cafc"],
  "scenario": {
    "params": { "rate": 850 },
    "payoffs": { "cofc::judgment-for-plaintiff": 4000000 },
    "objective": { "type": "cara", "a": 2e-7 },
    "fee_shift": { "fraction": 0.4 }
  },
  "op": { "op": "simulate", "runs": 50000 }
}'
```

Runnable requests live in [`examples/`](examples/) — a 28 U.S.C. § 1498
patent case in the Court of Federal Claims through the Federal Circuit:

| file | shows |
|---|---|
| `cofc-1498-chain.json` | expected cost/time/hours under the optimal policy |
| `cofc-1498-risk-averse-sim.json` | 50,000-run outcome simulation |
| `cofc-1498-tornado.json` | which inputs (rate, stakes) the answer is most sensitive to |
| `cofc-1500-trap-compare.json` | the § 1500 same-claim-pending trap as a counterfactual |
| `cofc-appeal-threshold-sweep.json` | the stake at which an appeal becomes worth briefing |
| `cofc-explain-dispositive-fork.json` | `explain` at a dispositive-motion decision point |

```bash
./target/release/litgraph q - < examples/cofc-1498-chain.json
```

## Install

```bash
# homebrew (macOS, Linux)
brew install davidawad/tap/litgraph

# from source
cargo install --git https://github.com/davidawad/litgraph litgraph-cli
cargo install --git https://github.com/davidawad/litgraph litgraph-mcp  # MCP server

# nix, no clone
nix run github:davidawad/litgraph -- describe

# container
docker run --rm ghcr.io/davidawad/litgraph describe

```

Prebuilt binaries (Linux x86_64 glibc/musl, macOS arm64/x86_64) and the
WebAssembly bundles are attached to each
[GitHub Release](https://github.com/davidawad/litgraph/releases) -- each
release tarball, the container image, and the nix flake all carry both
`litgraph` and `litgraph-mcp`. See [CONTRIBUTING.md](CONTRIBUTING.md) for
building from source with nix/devenv.

## Usage

### CLI

`litgraph q '<json>'` or `litgraph q - < request.json` (equivalently
`litgraph run request.json`) sends one request envelope and prints one
response envelope. Shorthand subcommands (`describe`, `packs`, `lint`,
`explain --packs ... --arg node=...`) build the envelope for you. `--packs`
accepts pack ids or `all` (default all). `litgraph schema <kind>` prints a
document's JSON Schema; `litgraph validate <file|->` checks a pack,
`links.json`, scenario, or request file (kind auto-detected, or forced with
`--kind`) and exits `2` if it's invalid. Exit codes throughout: `0` ok,
`2` the request failed or the document is invalid, `1` usage/I/O error.

### Library

The engine is a normal Rust crate (`litgraph`, in [`crates/litgraph`](crates/litgraph));
`litgraph-cli` is a thin binary over `litgraph::api::handle`. Add it as a
path or git dependency to embed the solver, the expression language, or the
pack loader directly — nothing about the CLI is required.

### MCP server

`litgraph-mcp` ([`crates/litgraph-mcp`](crates/litgraph-mcp)) is a stdio
[MCP](https://modelcontextprotocol.io) server: a thin wrapper over
`litgraph::api::handle`, same as the CLI. Every op is its own tool
(`solve`, `chain`, `explain`, ...), each with an input schema sliced
straight out of the engine's own JSON Schema for `Op` — plus one generic
`litgraph` tool that takes a raw request `{packs, links, no_continuations,
scenario, op}` verbatim. Packs, `links.json`, and the manual are MCP
resources (`litgraph://packs/<id>`, `litgraph://links`,
`litgraph://describe`). Every tool call returns the same JSON envelope as
the CLI; an engine error (`ok: false`) comes back as a tool error carrying
that envelope. `LITGRAPH_PACKS` and `--packs-dir` work exactly as they do
for the CLI.

**Claude Code:**

```bash
claude mcp add litgraph -- litgraph-mcp
# with a packs directory instead of the embedded set:
claude mcp add litgraph -e LITGRAPH_PACKS=/path/to/packs -- litgraph-mcp
```

**Claude Desktop** — add to `claude_desktop_config.json`
(`~/Library/Application Support/Claude/claude_desktop_config.json` on
macOS; `%APPDATA%\Claude\claude_desktop_config.json` on Windows;
`~/.config/Claude/claude_desktop_config.json` on Linux):

```json
{
  "mcpServers": {
    "litgraph": {
      "command": "litgraph-mcp"
    }
  }
}
```

**Any other MCP client** that reads the same `mcpServers` stdio shape
(Cursor, Windsurf, Zed, ...) uses the same block; add
`"args": ["--packs-dir", "/path/to/packs"]` or `"env": {"LITGRAPH_PACKS":
"/path/to/packs"}` to point at a pack set other than the ones embedded in
the binary.

### As the engine behind an application

[civ-pro-the-gathering](https://github.com/davidawad/civ-pro-the-gathering)
is a litigation-procedure training game and the proof-of-concept front end
for this engine: litgraph was extracted from its original TypeScript graph
library, rewritten in Rust, and verified node-for-node against the original
implementation (`crates/litgraph/tests/parity.rs`, golden fixtures in
[`tests/fixtures/ts-parity/`](tests/fixtures/ts-parity/)) before any
semantic changes were made. The game supplies the packs and the UI; litgraph
supplies the graph, the solver, and the JSON contract.

## Packs

Forum procedures are JSON files in [`packs/`](packs/); `packs/links.json`
joins them into one composed graph (e.g. a CoFC judgment linking to a
Federal Circuit appeal). They're compiled into the `litgraph` binary at
build time, so `litgraph packs` works right after install with no
`packs/` directory needed — point at a different set with
`LITGRAPH_PACKS=<dir>` or `--packs-dir <dir>`. Run `litgraph packs` for
live counts; schema is documented in
[`docs/PACK_SCHEMA.md`](docs/PACK_SCHEMA.md).

| pack | forum | schema | sourced (`sources` entries) | authored payoffs |
|---|---|---|---|---|
| `cofc` | Court of Federal Claims | v2 | 17, url + sha256 + as-of | 12/12 terminals |
| `cafc` | Federal Circuit | v2 | 8, url + sha256 + as-of | 18/18 terminals |
| `frcp-civil-procedure` | Federal Rules of Civil Procedure | v1 | none yet | heuristic fallback |
| `frap-appellate-procedure` | Federal Rules of Appellate Procedure | v1 | none yet | heuristic fallback |
| `frcrimp-criminal-procedure` | Federal Rules of Criminal Procedure | v1 | none yet | heuristic fallback |
| `itc-337` | ITC § 337 unfair-import investigations | v1 | none yet | heuristic fallback |
| `mpep-prosecution` | USPTO utility patent prosecution | v1 | none yet | heuristic fallback |
| `ptab-patent-trial-appeal-board` | PTAB trial proceedings | v1 | none yet | heuristic fallback |

v1 packs load and solve like any other pack — they carry authored
deadlines, hours, and (for most) probabilities — they just don't yet have
authored terminal payoffs or a `sources` list; the engine flags this on
every response (a grouped `payoff-not-authored` warning, plus an
unsourced-pack diagnostic from `lint`). Bringing a pack to v2 (sources +
authored payoffs) is exactly the kind of contribution described in
[CONTRIBUTING.md](CONTRIBUTING.md).

## Docs

- [AGENTS.md](AGENTS.md) — operating manual for agents (and anyone) driving
  the engine: commands, response shape, rules, module layout.
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) — the model (packs → graph →
  scenario → view → analyses → answers → accretion), design commitments,
  the composition model, roadmap.
- [docs/PACK_SCHEMA.md](docs/PACK_SCHEMA.md) — the pack JSON schema (v1 and
  v2), roles, links, authoring rules.
- [docs/COST_FUNCTIONS.md](docs/COST_FUNCTIONS.md) — the catalog of
  cost/time/risk/outcome/strategic functions, what's built in vs. one
  expression away vs. needs new engine capability.
- [docs/CRITIQUE.md](docs/CRITIQUE.md) — what was wrong with the original
  (v1) engine, the bugs found and fixed, what's still a known modeling
  limit.
- [CONTRIBUTING.md](CONTRIBUTING.md) — dev setup, quality gates, and the
  pack-authoring guide (every cite traceable to a source, "unauthored beats
  invented").

## License

GPL-3.0-or-later. See [LICENSE](LICENSE). Contributions are accepted under
the same license (inbound = outbound); see
[CONTRIBUTING.md](CONTRIBUTING.md) for the DCO sign-off convention.
