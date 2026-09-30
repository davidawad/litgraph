# How litgraph is tested

The Rust test code lives in `crates/litgraph/tests/` (integration tests, one
binary per file) and next to the code in `crates/litgraph/src/` (unit tests
in `#[cfg(test)]` modules). This directory holds the **data** the tests read:
readable fixtures and snapshots a reviewer can open without reading Rust.

```bash
cargo nextest run --workspace                          # everything
cargo test -p litgraph --test famous_cases             # just the famous cases
cargo test -p litgraph --test golden                   # just the golden graphs
UPDATE_EXPECT=1 cargo test -p litgraph --test famous_cases   # rewrite story snapshots
```

## The layers, from most readable to most thorough

| Layer | Data | Test | What it proves |
|---|---|---|---|
| **Famous cases** | `tests/cases/<slug>.json` + `<slug>.story.txt` | `famous_cases.rs` | Real, sourced procedural histories (Twombly, Celotex, eBay, Oil States, Arthrex, ...) are valid walks through the compiled packs, reach the right outcome, and get the right rule-computed deadlines. |
| **Golden graphs** | `tests/golden/<name>.json` | `golden.rs` | `solve`, `chain`, `simulate`, `path` and min-cut give the answer you get with a pencil on 2-6 node graphs. Each file shows its arithmetic. |
| **Features** | inline packs | `features.rs`, `general_sum.rs`, `cvar*.rs`, `path_vars.rs`, `calibration.rs`, `api_deadlines.rs`, `review.rs` | Each non-v1 capability behaves as documented on small hand-built graphs. `review.rs` pins defects an adversarial review found. |
| **Shipped content** | `packs/`, `scenarios/`, `calibration/` | `scenario_library.rs`, `calibration.rs`, `cite_coverage_report.rs` | Everything embedded in the binary still loads, resolves and cites. |
| **Parity** | `tests/fixtures/ts-parity/` | `parity.rs`, `clock_parity*.rs` | With v1 modes pinned, the engine reproduces the original TypeScript engine's numbers and deadline dates. |
| **Properties** | generated | `properties*.rs`, `clock_properties.rs` | Invariants over random graphs and dates (probabilities sum to 1, min cut = max flow, deadlines never land on a holiday, ...). |
| **Coverage gaps** | inline | `*_gaps*.rs`, `coverage_gaps.rs`, `api_coverage.rs`, `api_sweep_errors.rs` | Branches (error paths, degenerate inputs) the suites above don't reach; they keep line coverage at or above 97%. |
| **Proofs** | none | `src/proofs.rs` (Kani) | Properties proved for every input in a bounded space, not sampled. |

Read the table top-down to learn what the engine does. Read it bottom-up to
see how hard it has been pushed.

## Famous cases (`cases/`)

Each case file maps one real procedural history onto pack node and edge
ids, with a source for every fact:

- **`legs`**: runs of steps the packs can express. A step names the edge it
  takes (`"edge"`), or just where it lands (`"to"`) when only one edge gets
  there. The walker follows only out-edges of the node the case is on in
  the *compiled* graph, so `links.json` hops and state flags (IPR estoppel,
  a Federal Circuit panel's disposition) are exercised exactly as the
  algorithms see them.
- **`gaps`**: real moves no pack can make yet, such as a Supreme Court
  merits decision or an interlocutory appeal out of the district court.
  They are expected failures. The test asserts each one is *still* a gap,
  so the day a pack closes it the test fails and tells you to merge the
  legs.
- **`outcome`**: the node the history ends on, with its exact outcome tags
  and state flags.
- **`deadlines`**: key deadlines run through the real `deadlines` op. The
  expected date is the **rule's** date, worked by hand in `arithmetic`. It
  is *not* the day the parties actually filed; that goes in `historical`,
  for contrast.
- **`quirks`**: places where the pack forces a step the real case didn't
  take, or models an event differently. Not failures, but a reader should
  know about them.

Steps whose event isn't in the cited sources say **"route assumed"**. The
walk has to take some edge at every fork. When the record is silent, it
takes the ordinary one and says so rather than inventing a fact.

`<slug>.story.txt` is the `expect-test` snapshot of the rendered walk:
every event, the node it lands on, and the edge (with its authority and
deadline) that got it there. Review a pack change by reading the diff of
these files.

To add a case: write `cases/<slug>.json`, add a `famous_case!` line with a
sentence-style test name to `famous_cases.rs`, create an empty
`<slug>.story.txt`, and run with `UPDATE_EXPECT=1`.

## Golden graphs (`golden/`)

Each file holds one pack small enough to solve by hand, one op, the
expected slice of the op's `result`, and an `arithmetic` list showing how
each number was computed. Numbers must match to 1e-9. The exceptions are
sampled quantities from `simulate`, which get a `tolerance` of three
standard errors, also derived in `arithmetic`.

## Naming

Test names are sentences that state the behavior, e.g.
`suing_beats_settling_when_the_expected_verdict_exceeds_the_offer` or
`juneteenth_is_not_a_holiday_before_2021`. A failing test's name should
tell you what broke before you open the file.
