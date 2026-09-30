# NOTES: famous-case expect tests and clearer golden tests (lg-bfi)

## What was built

- **Ten famous cases** in `tests/cases/<slug>.json`, run by
  `crates/litgraph/tests/famous_cases.rs`:

  | Case | Forums walked | Exercises |
  |---|---|---|
  | Bell Atlantic v. Twombly (2007) | FRCP → FRAP | 12(b)(6) dismissal → appeal, 12(h) flag across a link |
  | Ashcroft v. Iqbal (2009) | FRCP; FRAP | collateral-order door, 60-day U.S.-officer window (and a documented gap between the two) |
  | Celotex v. Catrett (1986) | FRCP → FRAP | summary judgment → appeal |
  | eBay v. MercExchange (2006) | FRCP → CAFC (`cafc@district-court`) | post-trial motions, cross-appeal, `panel-mixed` |
  | Oil States v. Greene's (2018) | PTAB → CAFC (`cafc@ptab`) | `ipr-estopped`, Rule 36, July 4 roll |
  | SAS Institute v. Iancu (2018) | PTAB → CAFC | mixed FWD, en banc denial, partial-institution quirk |
  | United States v. Arthrex (2021) | PTAB → CAFC → (gap) → PTAB Director review → CAFC | CAFC remand, two appeals, `cert-denied` via `payoffByFlag` |
  | Hughes Aircraft v. United States (§ 1498) | CoFC → `cafc@cofc` → remand link → CoFC → `cafc@cofc-gov` → (GVR gap) → CAFC | both CoFC instances, remand link |
  | Blue & Gold Fleet v. United States | CoFC bid protest → `cafc@cofc` | protest track, 45-day U.S.-party rehearing |
  | Suprema v. ITC | ITC § 337 → `cafc@itc` | rehearing grant clears `panel-mixed`, then `panel-affirmed` |

  Each test asserts four things. The history is a valid walk of compiled
  out-edges (flags and links honored). Every documented gap is still a
  gap: an expected failure that flips once a pack closes it. The final
  node, outcome tags and flags are exact. And 22 key deadlines, computed by
  the real `deadlines` op, match hand-computed rule dates (not the
  historical filing dates, which are recorded alongside for contrast). The
  rendered story is an `expect-test` snapshot, `tests/cases/<slug>.story.txt`.
- **Seven golden graphs** in `tests/golden/`, run by
  `crates/litgraph/tests/golden.rs`: 2-6 nodes each, covering `solve` (2),
  `chain` (2), `simulate`, `path` (k-shortest) and min-cut. Each has an
  `arithmetic` list, is checked through the JSON contract to 1e-9, and
  uses a derived 3-standard-error tolerance for sampled `simulate` stats.
- **`tests/README.md`** explains the test layers, the case-file format,
  and the "route assumed" convention.
- Test names are sentence-style (e.g.
  `hughes_1498_case_is_remanded_for_damages_then_gvrd_then_affirmed`,
  `juneteenth_is_not_a_holiday_before_2021`).
- `examples/famous-case-noa-holiday-roll.json`, a CHANGELOG entry, the
  AGENTS.md layout, the ARCHITECTURE deadline-clock note, and a
  docs/CRITIQUE.md section listing the gaps.

## Gaps found and fixed (small, sourced)

1. **FRCP**: `dismissed-with-prejudice` and `summary-judgment-granted` are
   final decisions (28 U.S.C. § 1291) but could not be appealed. Added
   `dismissal-appeal-clock` and `summary-judgment-appeal-clock` into
   `notice-of-appeal-window`.
2. **FRAP**: added `no-transcript-certificate` (FRAP 10(b)(1)(B); the text
   is verified in `sources/frap.txt`) and `no-stay-sought` (FRAP 8(a)(1),
   mirroring CAFC's `motions-decline`).
3. **links.json**: `rehearing-time-expires` stayed at 30 days while
   `rehearing-file` was patched to 45 in `cafc@cofc`/`cafc@cofc-gov`.
   `cafc@itc` had neither patch, although the Commission is a U.S. agency
   party (Fed. Cir. R. 40(a)(1)(B); the text is verified in
   `sources/fedcir-rules.txt` and `sources/frap.txt`).
4. **Deadline clock (engine)**: Juneteenth and MLK Day were applied to
   every year. They now start in 2021 and 1986. Found by Suprema, where the
   op returned 2011-07-05 instead of 2011-07-01.

`lint` output is unchanged by the pack edits (81 diagnostics before and
after), and `validate` passes on all three files.

## Gaps listed, not fixed (too big for this change)

- **No Supreme Court merits stage.** Every `cert-granted` is terminal, so
  a GVR (Hughes) or a remand cannot return the case to the court below.
  A small SCOTUS pack plus links would close this for eight of the ten
  cases.
- **No interlocutory appeal out of the district court (Iqbal).** FRCP
  reaches FRAP only after a final judgment. Closing this needs a new FRCP
  node and a link into FRAP's interlocutory door.
- **No rehearing or cert after a CAFC vacate-and-remand (Arthrex).**
  `panel-to-remand` goes straight to a `remanded-to-*` terminal. The
  PTAB-side remand link also re-enters at `fwd-issued`, not at Director
  review.
- **PTAB deadlines are uncomputable.** `forum: "ptab"` maps to no clock
  rule set (37 C.F.R. § 1.7 / § 90.3(c) are not implemented).
- **Anachronisms.** PTAB forces a sur-reply and MTA preliminary guidance,
  and cannot express partial institution (pre-SAS). CAFC forces a bill of
  costs before the certiorari window. CAFC's `panel-to-mixed` has no
  remand branch. FRCP doesn't separate the permanent-injunction ruling
  (eBay). FRAP routes every panel-rehearing denial through an en banc
  poll. FRAP starts the cert window at the mandate instead of at the
  judgment or rehearing denial (Sup. Ct. R. 13.3). Outcome tags on
  `cert-granted` differ between packs (`remand` in CAFC, `pending` in
  FRAP).
- **The clock applies today's rule text.** Pre-2009 FRCP 6 short-period
  counting, and the Federal Circuit's historical rehearing period, are not
  modeled. The holiday calendar before 1971 (Uniform Monday Holiday Act)
  is today's.

Each case file's `quirks` list records the forced steps that apply to it.

## Deviations from the brief

- The brief names `tests/famous_cases.rs`. Cargo only compiles integration
  tests inside a crate, so the test binaries are
  `crates/litgraph/tests/famous_cases.rs` (plus `famous_cases/case.rs` and
  `render.rs`) and `crates/litgraph/tests/golden.rs`. The **data** lives
  where the brief put it, in the root `tests/` next to
  `tests/fixtures/ts-parity/`.
- **"Route assumed" steps.** The walk must take some edge at every fork.
  Where the sources are silent (reply briefs, oral argument, mediation
  screening, bill of costs), the step says "route assumed" rather than
  asserting a fact. Dates appear only where a source gives them.
- **New dependency**: `expect-test` 1.5 (dev-only; MIT/Apache-2.0; brings
  in `dissimilar`). It is the de facto inline/file snapshot crate, and
  `UPDATE_EXPECT=1` rewrites stories, so a reviewer diffs prose rather
  than asserts. `cargo deny check` passes: advisories, bans, licenses and
  sources are all ok.

## Gate results (toolchain 1.97.1, as pinned)

- `cargo fmt --all`: clean.
- `cargo clippy --workspace --all-targets -- -D warnings` (pedantic):
  clean.
- `cargo nextest run --workspace`: 510 passed, 1 skipped. Doctests pass.
- `cargo llvm-cov nextest --workspace --summary-only`: **97.60%** lines
  (workspace); `litgraph` crate **97.93%**.
- `cargo deny check`: ok (one pre-existing "Zlib allowance not
  encountered" warning).
- `typos`: clean for everything this change touches. It still reports six
  pre-existing hits in `marketing/graph/index.html` (two short JS identifiers), which
  came in with commit 179b92c and were left alone. `TRO` and `Markey` were
  added to `_typos.toml`.
- No `.rs` file over 500 lines (the largest new one is 285). Every new
  `.rs` file has an SPDX header and a `//!` doc. There is no
  `unwrap`/`expect` outside tests.

## Bead status

`lg-bfi` is left **open** in the local bead DB (canonical task; not closed
from this box). All of the brief's deliverables are done. The follow-ups
worth filing as beads are the "listed, not fixed" gaps above, starting
with a SCOTUS merits pack.

Research log: `/work/artifacts/case-research.md`.
