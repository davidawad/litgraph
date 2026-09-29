# Contributing to litgraph

Thanks for looking at litgraph. Contributions are welcome from humans and
agents alike — see [AGENTS.md](AGENTS.md) for the engine's operating manual
if you're driving it programmatically.

## Dev setup

Pick whichever fits your workflow; all three land in the same place.

```bash
# reproducible toolchain (nix flake, includes rust-toolchain.toml's pinned version + tools)
nix develop

# if you use devenv for shell management
devenv shell

# or just plain cargo, with the pinned toolchain from rust-toolchain.toml
cargo build
```

[`just`](https://github.com/casey/just) is the task runner — `just --list`
shows every recipe. The single gate to run before opening a PR:

```bash
just ci   # fmt-check, clippy -D warnings, nextest, doctests, coverage, audit, deny
```

CI runs the same recipe, so a clean `just ci` locally means CI should pass.

## Code standards

- **Formatting**: `cargo fmt --all` (`just fmt`); `just fmt-check` in CI.
- **Lints**: `cargo clippy --workspace --all-targets -- -D warnings`
  (`just lint`). Clippy pedantic is on workspace-wide with a short, commented
  allowlist in `Cargo.toml` for lints that fight this domain's own notation
  (short math names like `p`/`q`/`v`, exact float sentinel comparisons).
- **Tests**: `cargo nextest run --workspace` (`just test`), including
  doctests (`just doctest`, run separately — nextest doesn't execute them)
  and `proptest`-based property tests (co-located with the code they cover,
  run as part of `test`; `just proptest` filters to just those).
- **Coverage**: ≥97% line coverage on the `litgraph` crate (the pure-logic
  engine; the CLI crate is thin and not gated the same way):
  `just cov` (`cargo llvm-cov nextest -p litgraph --fail-under-lines 97`).
- **Formal verification**: Kani harnesses (`#[cfg(kani)]` / `#[kani::proof]`
  in `crates/litgraph`) for properties worth proving, not just testing —
  numerical invariants, no-panic guarantees on untrusted input. Run with
  `just verify-kani`. Zero harnesses today is a valid state (the recipe
  passes trivially); add one whenever you find a property proptest can only
  sample.
- **Supply chain**: `cargo audit` and `cargo deny check` (`just audit`,
  `just deny`) must pass — see `deny.toml` for the license/advisory policy.
- **File size**: keep source files under ~500 lines; split by
  responsibility (see `crates/litgraph/src/model/` for the pattern — a
  `mod.rs` plus `schema.rs`/`links.rs`/`graph.rs`/`resolve.rs`) rather than
  growing one file indefinitely.
- **Commits**: [Conventional Commits](https://www.conventionalcommits.org/)
  (`feat:`, `fix:`, `refactor:`, `docs:`, `chore:`, ...).
- **SPDX headers**: every `.rs` file starts with
  `// SPDX-License-Identifier: GPL-3.0-or-later`. If you add a new source
  file without one, add the header — `just lint`/CI will eventually enforce
  this, but don't wait for the check to catch it.
- **License**: GPL-3.0-or-later, inbound = outbound — by contributing you
  agree your changes are licensed under the same terms. Sign off every
  commit (Developer Certificate of Origin):

  ```bash
  git commit -s -m "feat: ..."
  ```

  `-s` appends a `Signed-off-by: Your Name <you@example.com>` trailer
  attesting you have the right to submit the change under the project's
  license.

## Pack authoring guide

Packs (`packs/*.json`) are the highest-value place to contribute — they're
also the place where a wrong number does the most damage, so the bar is
specific. Read [docs/PACK_SCHEMA.md](docs/PACK_SCHEMA.md) first; this
section is the rules distilled.

1. **Target schema v2.** `schemaVersion: 2` packs carry authored terminal
   `payoff`/`outcome`, a `sources` list, `roles`, and `attrs`. v1 packs
   (the original civ-pro-the-gathering statecharts) still load and solve,
   but every terminal falls back to the engine's label-heuristic payoff,
   which is flagged on every response. Porting a v1 pack to v2 — adding
   `sources`, authored payoffs, and outcome tags without changing any node,
   edge, deadline, or existing probability — is a great first PR.

2. **Every cite is traceable.** Every `authority`/`cite` string must
   resolve to an entry in the pack's `sources` array (or be a bare public
   citation an agent/reviewer can verify independently — a U.S.C. or CFR
   section, a rule number). Each `sources` entry needs:
   - `url` — the official, currently-live source (court/agency site,
     Cornell LII, GovInfo — not a personal mirror or local file path).
   - `sha256` — the hash of the document you actually read, so a future
     re-download can be checked against what the pack was authored from.
   - `asOf` — the date you captured it (rules change; this is how a reader
     knows whether a cited deadline might be stale).

   Never invent a rule number, statute section, or deadline. If you're not
   sure a fact is right, mark it `UNVERIFIED` in the node/edge `note`
   rather than asserting it.

3. **Probabilities are optional, and only on non-`self` edges** (edges the
   protagonist doesn't choose — court/agency/chance outcomes). Where you
   author one, put the basis in `note`: a docket statistic with its vintage
   ("PTAB institution rate, FY2024 AIA stats"), or explicitly "teaching
   estimate" if it's a pedagogical guess, not data. **Unauthored beats
   invented** — an omitted probability gets the engine's documented
   residual-fill treatment and a warning; a made-up one looks authoritative
   and isn't. Out-edges of a fully-authored chance node must sum to 1.

4. **Cross-forum composition** goes in `packs/links.json`, not by editing a
   terminal's payoff to fake a continuation. If a forum needs to remember
   how it was entered (different remand routing, a perspective flip, a
   payoff transform depending on origin — see `cafc@cofc` vs. `cafc@itc` in
   the current links), add a named **instance** rather than branching
   inside the shared pack.

5. **Validate before you open a PR:**

   ```bash
   litgraph validate packs/<your-pack>.json   # parses? refs resolve? (exit 2 if not)
   litgraph lint --packs <your-pack-id>       # content diagnostics
   cargo test -q                              # unit + parity + your pack loads cleanly
   ```

   `validate` catches a malformed document (parsing is strict — an unknown
   field is an error naming the valid ones) and unresolved references
   before you even get to content quality. `lint` will name unsourced
   packs, unquantified chance nodes, missing payoffs, and dead ends — fix
   what it finds or explain in the PR why the gap is intentional (a
   genuinely unauthored area of law, for instance).

6. **Don't change ids, deadlines, probabilities, or payoffs incidentally.**
   If a PR's stated purpose is "add sources", the diff should be sources
   (and `path`→`url`/`sha256` cleanups) — not renumbered deadlines or
   rebalanced probabilities bundled in. Split those into a separate PR with
   its own citation.

## Getting started

Check the [GitHub Issues](https://github.com/davidawad/litgraph/issues) list
for open work; `AGENTS.md` for the CLI commands and response contract;
`docs/ARCHITECTURE.md` for how the pieces fit together before you change one
of them.
