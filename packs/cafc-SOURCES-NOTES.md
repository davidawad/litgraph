# cafc-federal-circuit.json — sources & authoring notes

## What was read

- **Schema**: `docs/PACK_SCHEMA.md` (v2 fields, roles convention, authoring rules 1-4).
- **Model packs**: `packs/frap-appellate-procedure.json` (style/density model; this pack is the
  Federal-Circuit-specific counterpart, not a copy — Fed. Cir. local rules override or supplement
  FRAP defaults throughout: docketing fee, brief deadlines/word limits, appendix contents,
  certificate of interest, docketing statement, Rule 36, Rule 40's 30/45-day rehearing window, etc.),
  `packs/ptab-patent-trial-appeal-board.json` and `packs/itc-337.json` (their `federal-circuit-window`
  / `federal-circuit-appeal-window` nodes are the handoff points this pack's `entry-ptab` and
  `entry-itc` nodes represent from the CAFC side).
- **Primary sources**, all downloaded 2026-09-28 from `www.cafc.uscourts.gov`, read with
  `pdftotext -layout`:
  - `federal-circuit-rules-of-practice-2025-12-01.pdf` — the court's single combined compilation of
    FRAP text + Federal Circuit Rules + Practice Notes (sha256
    `b6346ed56a0ef66e8a681765873caf69239cda273f8946f1625571e013712c05`). This is where nearly every
    rule cite in the pack comes from; line numbers below refer to the `pdftotext -layout` output.
  - `federal-circuit-internal-operating-procedures-2026-06-09.pdf` (sha256
    `0529dbba781897e83ed617af7a603f7fdaf7ac4af1278d035a378c5699172397`) — IOP #7 (oral argument time
    allocation and the frivolous/authoritatively-decided/adequately-presented no-argument criteria),
    IOP #8 (panel conference, straw vote, Rule 36 vs. opinion choice), IOP #9 (disposition vehicles).
  - `appellate-mediation-program-guidelines-2013-12-06.pdf` (sha256
    `b70b1463ebd122dbe3287d5c7117e822f5bfe32a59a53bb9b5b5e000b7a1dfb0`) — mandatory-if-selected
    participation, OGC screening off the docketing statement + briefs, confidentiality, volunteer
    mediator roster. No published selection rate or settlement rate exists in this document.
- **Web-verified** (WebSearch/WebFetch on official/near-official sources, listed as `sources` entries
  without a local path):
  - CAFC Fee Schedule (`cafc.uscourts.gov/wp-content/uploads/FeeSchedule.pdf`, dated Dec 1, 2023,
    fetched and `pdftotext`'d directly): docketing/filing fee for a district-court-origin appeal is
    **$605** (paid to the district clerk); docketing fee for a "petition for review" (the PTAB/ITC
    appeal vehicle) is **$600** (paid to the CAFC clerk). CoFC-origin appeals are not separately
    itemized on this schedule — the pack uses $605 as a reasonable but **UNVERIFIED-to-the-cent**
    proxy, flagged on that edge's `note`.
  - 37 C.F.R. § 90.3 (63-day PTAB notice-of-appeal window, filed with the Director, extendable only
    by the Director on a timely good-cause or post-hoc excusable-neglect showing) — cross-checked
    against eCFR/Cornell via WebSearch; matches `ptab-patent-trial-appeal-board.json`'s
    `federal-circuit-window` node.
  - 28 U.S.C. § 2522 / the CoFC 60-day notice-of-appeal deadline — cross-checked via WebSearch
    (uscode.house.gov, Justia).
  - 28 U.S.C. § 1295(a) subsections — cross-checked against the statute text (uscode.house.gov) for
    (a)(1) district court patent, (a)(3) CoFC, (a)(4)(A) PTAB, (a)(6) ITC, per the team-lead brief.
  - Rule 36 usage rate: Jason Rantanen, "Federal Circuit Decisions - 2024 Stats and Datapack,"
    Patently-O (June 2025) — ~20% of 2024 merits terminations were Rule 36 summary affirmances;
    43% of PTAB-origin appeals 2011-2024; 67% vs. 18% depending on whether the patent owner or the
    petitioner is the appellant on a PTAB appeal. Only the ~20% overall figure is used as the
    authored basis for `panel-decision`'s Rule 36 edge; the PTAB-specific skew is noted but not
    separately modeled (this pack doesn't branch `panel-decision` by origin).
  - SCOTUS-wide ~1%/99% cert grant/deny split — well-established public figure (rule of four, Sup.
    Ct. R. 10), not Federal-Circuit-specific; reused from `frap-appellate-procedure.json`'s identical
    figure rather than re-deriving.

## Where each major node/edge comes from

| Node/edge | Rule/statute | Source |
|---|---|---|
| `entry-district-court`, `entry-ptab`, `entry-itc`, `entry-cofc` | 28 U.S.C. § 1295(a)(1)/(4)(A)/(6)/(3) | statute (web-verified) |
| District-court NOA deadline (30/60 days) | FRAP 4(a)(1)(A)-(B) | rules PDF (FRAP text), lines ~787-831 |
| PTAB NOA deadline (63 days, filed with Director) | 37 C.F.R. § 90.3(a), (c); 35 U.S.C. § 142 | WebSearch (eCFR/Cornell); mirrors ptab pack |
| ITC NOA deadline (60 days, unextendable) | 19 U.S.C. § 1337(c) | mirrors itc-337.json's `appeal-filed` edge verbatim |
| CoFC NOA deadline (60 days) | 28 U.S.C. § 2522; FRAP 4(a)(1)(B) | WebSearch; US is always a party to a CoFC judgment so the 60-day FRAP period applies as a matter of course |
| Docketing fee $605 / $600 | Judicial Conference Court of Appeals Misc. Fee Schedule, per CAFC Fee Schedule (Dec 1, 2023) | web-fetched PDF |
| Entry of appearance, 14 days | Fed. Cir. R. 47.3(b)(1) | rules PDF lines ~8877-8916 |
| Certificate of interest, filed contemporaneously with first appearance | Fed. Cir. R. 47.4(a)-(c) | rules PDF lines ~8966-9020 |
| Docketing statement, 14 days, Form 26 | Fed. Cir. R. 47.6 | rules PDF lines ~9096-9114 |
| Mediation program (mandatory if selected, uses docketing statement) | Appellate Mediation Program Guidelines §§ 1-7; Fed. Cir. R. 33(b) | mediation PDF; rules PDF lines ~6965-6980 |
| Stay pending appeal | FRAP 8(a)-(b); Fed. Cir. R. 8(a)-(c) (certificate of interest + trial-court-first requirement) | rules PDF lines ~1290-1389 |
| Cross-appeal deadline (14 days, general FRAP rule) + CAFC-specific page/word/color-cover limits | FRAP 4(a)(3); Fed. Cir. R. 28.1(a)-(f) | rules PDF lines ~5126-5365 |
| Appellant's principal brief, 60 days after docketing/certified list | Fed. Cir. R. 31(a)(1)(A)-(B) | rules PDF lines ~6286-6320 (local 60-day extension of the FRAP default 40 days, footnoted at line ~6235) |
| Appellee's brief, 40 days | Fed. Cir. R. 31(a)(2) | rules PDF lines ~6313-6320 |
| Reply brief, 21 days / ≥7 days before argument (unmodified FRAP default) | FRAP 31(a)(1) | rules PDF lines ~6219-6230 |
| Dismissal for failure to prosecute (no opening brief) | Fed. Cir. R. 31(d) | rules PDF lines ~6369-6372 |
| Joint appendix, 7 days after last reply brief; contents; designation timeline | Fed. Cir. R. 30(a)(1)-(2), (b)(1) | rules PDF lines ~5755-5966 |
| Word limits (14,000/7,000 ordinary; 14,000/16,500/7,000 cross-appeal) | Fed. Cir. R. 32(b)(1); Fed. Cir. R. 28.1(b) | rules PDF lines ~6674-6680, ~5293-5309 |
| Oral argument vs. submission criteria | FRAP 34(a)(2); Fed. Cir. IOP #7 | rules PDF lines ~6995-7014; IOPs lines ~474-498 |
| Rule 36 judgment of affirmance without opinion | Fed. Cir. R. 36(a)-(b) | rules PDF lines ~7347-7382 |
| Rule 36 usage rate (~20% overall, teaching-estimate basis for the rest) | Patently-O 2024 Datapack | web search, cited above |
| Rehearing / rehearing en banc, 30 days (45 if US party), combined-petition practice | Fed. Cir. R. 40(a)(1)-(2), (f) | rules PDF lines ~7776-7955 |
| Mandate, issues 7 days after rehearing period/denial | FRAP 41(a)-(b); Fed. Cir. R. 41 | rules PDF lines ~8087-8154 |
| Costs, 14-day bill of costs, allocation-follows-result | FRAP 39(a)-(f); Fed. Cir. R. 39(a)-(c) | rules PDF lines ~7449-7575 |
| Cert petition, 90 days | 28 U.S.C. § 2101(c); Sup. Ct. R. 13.1, 13.3 | rules PDF (FRAP 41(d) practice note on cert stays, lines ~8106-8168) + well-established public citation |
| `remanded-to-*` terminal ids | (structural, per team-lead brief) | not rule-derived; ids chosen for composition into origin packs |

## Payoff/probability conventions (illustrative, flagged where unsourced)

- $1,000,000 illustrative stake from the appellant's perspective, per the authoring brief:
  affirmed-for-appellee (including Rule 36) = **0**; outright reversal = **+1,000,000**; any
  vacate-and-remand = **+400,000**; mixed result = **+250,000** (interpolated, not independently
  sourced); mediated settlement = **+350,000** (interpolated, not independently sourced).
- `panel-decision`'s six-way split sums to 1.00 (0.20/0.28/0.10/0.20/0.17/0.05). Only the 0.20 Rule
  36 figure is independently sourced (Patently-O 2024 Datapack); the other five are teaching
  estimates apportioning the remainder, explicitly flagged as such in each edge's `note` — consistent
  with `ptab-patent-trial-appeal-board.json` and `itc-337.json`'s own "APPROXIMATE" convention for
  unsourced splits rather than leaving the node fully unauthored (Rule 3's "must sum to 1.0"
  requirement for the node the team lead specifically called out).
- `remand-destination`'s 0.25/0.25/0.25/0.25 split is explicitly **structural**, not statistical — the
  real destination is determined by which `entry-*` node the playthrough started from, not by chance;
  documented in both the node's `note` and each edge's `note`.
- `mediation-screening`, `mediation-outcome`, and `oral-argument-fork` are **intentionally left with no
  probability at all** on any of their edges (not even a rough estimate) — no selection rate,
  settlement rate, or CAFC-specific oral-argument rate could be verified in the sources reviewed, and
  rule 2 of the schema's authoring rules says unauthored beats invented. The engine's uniform-draw
  fallback (and its `warnings`/`assumptions` reporting) is the intended behavior at these nodes.
- Rehearing/en-banc combined grant rate (0.93/0.07) and cert grant/deny (0.99/0.01) are carried over
  from `frap-appellate-procedure.json`'s cross-circuit figures rather than re-derived, since no
  CAFC-specific published rate was found for either; flagged in each edge's `note`.

## Things left out or marked UNVERIFIED

- CoFC-origin docketing fee (used the $605 district-court figure as a proxy — flagged).
- Any numeric selection/settlement rate for the mediation program (left unauthored, not estimated).
- Any CAFC-specific oral-argument-vs-submission split (left unauthored, not estimated; the ~19-25%
  cross-circuit AO figure found in search results was deliberately NOT used since it isn't
  Federal-Circuit-specific and the Federal Circuit's patent docket is generally believed to argue at a
  different rate than the circuit average, with no verified number to replace the guess).
- Federal Circuit Attorney Discipline Rules (bundled into the same combined PDF) — not modeled; out of
  scope for an appellate-procedure pack.
- Highly Sensitive Documents procedures (Administrative Order 2021-04) and the joint courthouse
  security order (2024-02) — read the source list but not modeled; not part of the appeal's
  procedural spine.

## Validation performed

Ran a Python validator (parses JSON; every edge's `from`/`to` resolves to a real node id; all 71 edge
ids are unique; `startNodeId` (`appeal-origin`) exists; every non-terminal node has ≥1 outgoing edge;
every node is reachable from `startNodeId` via a BFS over the edge list; every chance node where *any*
out-edge carries a `probability` has *all* out-edges carrying one, and those sum to 1.00 ± 0.01; no
edge with `actor: "applicant"` carries a `probability`, per the `self`-edges-never-probabilistic rule).

Result: **58 nodes, 71 edges, 0 errors, 0 warnings.**

Node kind breakdown: 13 decision, 27 state, 18 terminal.
