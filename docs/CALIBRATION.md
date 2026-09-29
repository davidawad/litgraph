# Calibration

A pack's `probability`/`duration` fields are often authored as "teaching
estimates" — plausible numbers with a `note` saying so, not real data.
Calibration replaces specific ones with sourced values from published
statistics, without editing the pack: **data plus mechanism**, the same
split packs themselves use.

## The file format: `calibration/*.json`

A calibration set is one JSON file, embedded into the binary at build time
exactly like `packs/*.json` (see `crates/litgraph/build.rs`), and
overridable at runtime the same way packs are: `$LITGRAPH_CALIBRATION` (or
the CLI's `--calibration-dir`) points at a directory of `*.json` files
instead. `litgraph schema calibration` gives the JSON Schema;
`litgraph validate my-set.json` (or `--kind calibration`) checks a file's
shape and resolves every `ref` against the loaded packs.

```jsonc
{
  "id": "ptab-fy2024",              // the name a scenario opts in with
  "title": "PTAB IPR/PGR institution and FWD rates, FY2024",
  "description": "...",             // what this set covers, in prose
  "entries": [
    {
      "target": "edge-probability", // edge-probability | edge-duration | node-attr
      "ref": "ptab-patent-trial-appeal-board::institution-decision->institution-granted#0",
      "value": 0.6810,              // required for edge-probability / node-attr
      "source": {
        "title": "PTAB Trial Statistics FY24 End of Year Outcome Roundup",
        "url": "https://www.uspto.gov/sites/default/files/documents/ptab_aia_fy2024__roundup.pdf",
        "vintageStart": "2023-10-01",
        "vintageEnd": "2024-09-30",
        "retrieved": "2026-09-28"
      },
      "n": 1087,                    // sample size / population count
      "method": "740 instituted / (740 + 347 denied) = 0.6810"
    }
  ]
}
```

| field | notes |
|---|---|
| `target` | `edge-probability` (sets `value`, 0..=1), `edge-duration` (sets `distribution`, a `Duration` — `{min?, mode, max?}` days), or `node-attr` (sets `attr` + `value`) |
| `ref` | node or edge ref, same resolution as everywhere else in the engine (qualified `pack::local` recommended). An instance-qualified ref like `cafc@cofc::panel-to-affirmed` targets one `links.json` pack instance only, not the base pack or its siblings. If the target pack uses [state flags](PACK_SCHEMA.md#state-flags), a bare ref (no `{flag}` suffix) resolves to that edge's *empty-flag* copy only — a flagged sibling reached with history already attached (e.g. `ptab-patent-trial-appeal-board.json`'s `fwd-issued->fwd-all-unpatentable#0{ipr-estopped}`, reachable after a Director-review remand for further proceedings) keeps its authored default rather than the calibrated value. Author a second entry with an explicit `{flag}`-suffixed ref if that path also needs the calibrated number |
| `source` | `title`, `url` (fetched and checked to resolve when the entry was authored), `vintageStart`/`vintageEnd` (the reporting period), `retrieved` (when you checked it) |
| `n` | sample size, or the count itself for a full-population annual total |
| `method` | required whenever the value is derived arithmetic from the source's raw counts (show the arithmetic) |

Authoring rules (same spirit as `docs/PACK_SCHEMA.md`):

1. Every number must come from a document you actually fetched, with the
   URL recorded and checked to work. Never invent a number.
2. If a source gives raw counts and only a derived rate is usable, show the
   arithmetic in `method`.
3. If you can't cleanly map a source's category onto the graph's edges
   (e.g. a source lumps two outcomes the pack models as separate edges),
   leave that edge uncalibrated rather than guessing a split. The `lint`
   warnings (`probability-fill`, `probability-renormalized`) and the
   `calibration` op both surface what's still unauthored.
4. Fewer, solid numbers beat many soft ones.

## Applying a set: `scenario.calibration`

```json
{
  "packs": ["ptab-patent-trial-appeal-board"],
  "scenario": { "calibration": ["ptab-fy2024"] },
  "op": { "op": "chain" }
}
```

Applying a set mutates the *compiled graph*, not the scenario's usual
override maps: each entry's value becomes **authored**, exactly as if it
had been hand-typed into the pack. That means the existing engine machinery
handles everything downstream for free — probability-fill for any sibling
edges the set didn't reach, `probability-renormalized` if the authored sum
drifts from 1.0, duration fallbacks, all unchanged. A ref that doesn't
resolve (its pack wasn't loaded in this request) is silently skipped, the
same convention `links.json` uses for a link into an unloaded pack — a
calibration set is expected to span multiple forums and be used with any
subset of packs.

`scenario.calibration` is a list: `["ptab-fy2024", "cafc-fy2024"]` applies
both, in order (a later set overwrites a ref an earlier one also set).

## Provenance: what was calibrated

Every response's `provenance.calibration` says exactly what happened,
whether or not the request asked for calibration (an empty array if not):

```jsonc
"calibration": [
  {
    "set": "ptab-fy2024",
    "applied": [
      {
        "target": "edge-probability",
        "ref": "ptab-patent-trial-appeal-board::institution-decision->institution-granted#0",
        "value": 0.681,
        "n": 1087,
        "sourceTitle": "PTAB Trial Statistics FY24 End of Year Outcome Roundup",
        "sourceUrl": "https://www.uspto.gov/sites/default/files/documents/ptab_aia_fy2024__roundup.pdf",
        "vintageStart": "2023-10-01",
        "vintageEnd": "2024-09-30"
      }
      // ...
    ],
    "skipped": 0
  }
]
```

An agent reading a response can always tell an authored-from-real-data
number from a teaching estimate or an engine fallback, without reading the
pack source.

## Finding what to calibrate next: the `calibration` op

```json
{ "packs": ["frcp-civil-procedure"], "op": { "op": "calibration", "top": 10 } }
```

Ranks every **uncalibrated** probability and duration by decision
sensitivity, using the same one-at-a-time perturbation method `tornado`
uses for authored inputs (`crate::algo::sweep`), just aimed the other way:

- **probabilities**: perturbed by ±`dp` (default 0.1), ranked by
  `|ΔV(start)|` — how much the scenario's dollar value swings.
- **durations**: perturbed by ±`rel` (default 0.25, relative to the edge's
  deadline-length fallback, or a fixed 0–30 day band absent one), ranked by
  `|Δ expected elapsed days|` to absorption under the optimal policy.

`max_candidates` (default 300) bounds how many edges of each kind are
actually solved for, so a large multi-pack selection stays cheap; the
result says `probabilities_truncated`/`durations_truncated` when it had to
stop early rather than silently returning a partial ranking.

The two lists use different units (dollars vs. days) and are reported
separately rather than force-ranked into one list — see the source comment
in `crates/litgraph/src/api/calibration/gaps.rs` for why.

## The first calibration set

`calibration/*.json` currently ships four FY2024 sets, each sourced from one
fetched, working document:

| file | covers | source |
|---|---|---|
| `ptab-fy2024.json` | IPR/PGR institution rate; FWD outcome split (all-unpatentable / mixed / all-confirmed) | USPTO PTAB Trial Statistics FY24 End of Year Outcome Roundup |
| `cafc-fy2024.json` | `cafc@cofc` affirmance and vacatur/remand rates | CoFC FY2024 Statistical Report, "Appeals to the ... Federal Circuit" |
| `cofc-fy2024.json` | annual bid-protest filing/disposition volume (node attrs) | same CoFC report |
| `frcp-fy2024.json` | district-court time-to-disposition (voluntary dismissal, summary judgment) | AO Judicial Business Table C-5 |

Deliberately left uncalibrated (real gaps, not oversights — good first
targets for the next pass, and exactly what `litgraph q -` with
`{"op":"calibration"}` will surface):

- PTAB institution-denial reason (merits vs. Fintiv discretion): the source
  reports only a combined "denied" total.
- Federal Circuit Rule 36 rate, and the `reversed` vs.
  `affirmed-in-part-reversed-in-part` split for CoFC-origin appeals: no
  primary source with matching granularity was independently verified in
  this pass (a `panel-to-rule36` teaching estimate already cites
  `patentlyo-2024-datapack`; that citation was not re-verified here and is
  left as-is rather than duplicated into a calibration entry).
- CoFC bid-protest outcome rate (sustained vs. denied): the fetched
  statistical report gives filing/disposition volume only, not outcomes.

## Regenerating entries: `scripts/calibrate/`

`scripts/calibrate/build_frcp_fy2024.py` regenerates `frcp-fy2024.json`'s
duration entries from the constants recorded in the script (the AO table's
published months, converted to days) — a worked example of "show your
work" for a derived number, runnable offline with no network access. It is
optional scaffolding, not a scraper: the other three sets were transcribed
by hand from the fetched PDFs and are not regenerated by a script.
