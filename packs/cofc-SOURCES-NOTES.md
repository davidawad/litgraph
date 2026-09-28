# CoFC pack -- sources and authoring notes

`cofc-court-of-federal-claims.json`, schemaVersion 2, forum `cofc`, id `cofc`,
startNodeId `claim-accrues`. 68 nodes / 84 edges / 12 terminals; validated
(script and output below).

## What was read

- **RCFC main rules PDF** -- `.../Court of Federal Claims/rcfc-rules-2026-07-27.pdf`
  (as amended through 7/27/2026), read via `pdftotext -layout` in full
  (13,162 lines) plus targeted grep passes for Appendix A, C, D, E and the
  fee-schedule cross-reference. Also cross-read the same rule text pre-split
  by rule number at `/Users/david/Agents/kb/scratch/rcfc_chapters/` (index
  `_index_list.txt`) for RCFC 3, 4, 12, 14, 16, 41, 54, 55, 56, 58, 58.1, 59.
- **RCFC Appendix J (Patent Rules)** -- `rcfc-appendix-j-patent-rules-2026-07-27.pdf`,
  read in full via `pdftotext -layout` (Rules 1-24, all deadlines).
- **RCFC Appendix H (ADR)** -- `adr-procedures-appendix-h.pdf`, read in full.
- **RCFC Appendix A (Case Management Procedure)** and **Appendix C (Procurement
  Protest Cases, 28 U.S.C. §1491(b))** -- read via `sed`/`grep` extraction from
  the main rules PDF's full text dump (Appendix A ¶1-13; Appendix C ¶1-17+).
- **Appendix of Forms**: Form 4 (Bill of Costs), Form 5 (EAJA application), and
  Form 8A (Patent Protective Order) -- first pages/captions read via
  `pdftotext -layout` to confirm form numbers/titles cited in node notes.
- **`.../SOURCES.md`** in the same KB directory -- confirmed sha256 hashes and
  official-source provenance (downloaded 2026-09-28 directly from
  uscfc.uscourts.gov) for every RCFC/Appendix PDF cited above.
- **U.S. Court of Federal Claims Schedule of Fees** (effective 7/28/2025) --
  fetched from `https://www.cfc.uscourts.gov/sites/cfc/files/25.07.28%20UPDATED%20Fee%20Schedule.pdf`
  via WebSearch + WebFetch, then `pdftotext`'d directly (the model pack's own
  FRCP fee figure was flagged "APPROXIMATE... not in local corpus"; this pack
  instead verifies the CoFC-specific figure against the court's own current
  schedule: **complaint/petition filing fee $405.00, includes a $55.00
  general administrative fee**, waived IFP for non-prisoner plaintiffs under
  28 U.S.C. §1915; notice-of-appeal fee $605.00, not separately modeled since
  the appeal itself lives in the future CAFC pack).
- **Statutes** verified via WebSearch against law.cornell.edu / govinfo /
  uscode.house.gov text: 28 U.S.C. §§1491 (Tucker Act), 1498(a) (patent/copyright
  use -- exclusive remedy, "reasonable and entire compensation," no
  injunction), 1500 (same-claim-pending bar; confirmed against *United States
  v. Tohono O'odham Nation*, 563 U.S. 307 (2011)), 2501 (6-year limitations,
  jurisdictional per *John R. Sand & Gravel Co. v. United States*, 552 U.S. 130
  (2008)), 2503, 2522 (60-day notice of appeal), 1295(a)(3) (CAFC
  jurisdiction), 2412(d)(1)(B) (EAJA, 30-day application window, "final
  judgment" defined at §2412(d)(2)(G)), and 35 U.S.C. §286 last paragraph
  (administrative-claim tolling, up to 6 years).
- The model packs `frcp-civil-procedure.json` and `frap-appellate-procedure.json`
  for density/style conventions (both are actually schemaVersion 1 in the repo
  today -- neither uses `roles`/`sources`/v2 fields yet, so this pack is the
  first to populate those v2 fields; `PACK_SCHEMA.md` was the authority for
  their shape, not the existing packs).

## What node/edge maps to which rule (high-value ones; full detail is in each
node's own `note`/`cite`)

| Node/edge | Rule |
|---|---|
| `claim-accrues` → `limitations-check` | 28 U.S.C. §2501 |
| `patent-admin-claim-filed`/`-resolved` | 35 U.S.C. §286, last paragraph |
| `section-1500-check` | 28 U.S.C. §1500; *Tohono O'odham* |
| `filing-track-fork` → bid-protest branch | 28 U.S.C. §1491(b); RCFC Appendix C |
| `complaint-filed` (fee $405) | RCFC 3; RCFC 5.5; court fee schedule eff. 7/28/2025 |
| `service-on-united-states` | RCFC 4(a)-(c) (clerk serves the AG, not plaintiff) |
| `government-response-fork` (60-day answer) | RCFC 12(a)(1)(A) |
| `ruling-12b` outcomes | RCFC 12(b), 12(a)(4), 12(h)(3) |
| `voluntary-dismissal-early` (two-dismissal rule) | RCFC 41(a)(1)(A)-(B) |
| `case-type-fork` | RCFC Appendix J, Rule 1(a) |
| `general-jpsr-filed` (49 days) | RCFC 16(b)(1)(A); Appendix A ¶4 |
| `summary-judgment-motion-filed` (30 days post-discovery) | RCFC 56(a)-(b) |
| `adr-*` nodes | RCFC Appendix H ¶2-3 |
| `patent-14b-notice-fork/-served` | RCFC 14(b)-(c) (very important in §1498 cases -- brings the actual contractor in) |
| `patent-jpsr-filed` (49/98 days) | RCFC Appendix J, Rule 3 |
| `patent-protective-order-entered` | RCFC 26(c)(1); PRCFC Rule 19(b); Form 8A |
| `patent-infringement-contentions` (56 days after answer) | Appendix J, Rule 4-5 |
| `patent-invalidity-contentions` (56 days after that) | Appendix J, Rule 6-7 (Rule 8 rescinded) |
| `patent-claim-construction-exchange` (42 then 28 days) | Appendix J, Rule 9-10 |
| `patent-joint-claim-construction-chart` (35 days) | Appendix J, Rule 11-12 |
| `patent-claim-construction-discovery-closed` (28 days) | Appendix J, Rule 13 |
| `patent-claim-construction-hearing` (7 then 90 days) | Appendix J, Rule 14-15 |
| `patent-mandatory-settlement-discussion` (14 days) | Appendix J, Rule 16-17 |
| `patent-final-contentions-expert-discovery` | Appendix J, Rule 23-24; RCFC 26(a)(2) |
| `trial-bench` (no jury, ever) | RCFC 38/39 "[Not used]"; RCFC 52 |
| `post-trial-motion-window` (28 days, unextendable) | RCFC 59(a)-(b), 59(e); RCFC 6(b) |
| `costs-and-fees-window` (30/30 days) | RCFC 54(d); 28 U.S.C. §2412(d)(1)(B) & (d)(2)(G); Forms 4 & 5 |
| `judgment-entered` (Federal Circuit composition anchor) | RCFC 56(a); RCFC 58; kept at this exact id per the task's pack-authoring instruction so a future `packs/links.json` can attach `cofc::judgment-entered → cafc::notice-of-appeal-filed` (28 U.S.C. §1295(a)(3); §2522, 60-day window since the U.S. is a party) |

## Design choices / things flagged as UNVERIFIED or placeholder

- **All `probability` values are marked "TEACHING ESTIMATE"** in their edge
  `note` -- none come from a cited empirical CoFC statistic (motion-grant
  rates, bid-protest sustain rates, ADR settlement rates, bench-trial win
  rates). The task's own instructions accept "teaching estimate" as a valid
  basis when flagged; none were invented without that flag. The bid-protest
  sustain-rate figure (0.20/0.80) in particular is explicitly marked
  UNVERIFIED against a primary statistical source -- publicly reported
  GAO/CoFC protest sustain rates exist but were not pulled and cite-checked
  in this pass.
- **`judgment-entered` vs `judgment-for-plaintiff`/`judgment-for-government`**:
  the task asked for all of `summary-judgment-for-government`,
  `judgment-for-plaintiff`, `judgment-for-government`, AND a `judgment-entered`
  terminal reachable by that exact id for CAFC composition. To keep every
  terminal a true sink (no node of kind `terminal` has an out-edge) while
  still giving all four a distinct, sourced procedural meaning: `judgment-entered`
  is reached specifically via a **full RCFC 56 summary-judgment grant for
  plaintiff** (liability and damages both resolved, no trial needed), while
  `judgment-for-plaintiff`/`judgment-for-government` are reached via an
  **actual bench-trial verdict** that survives RCFC 59 post-trial practice and
  RCFC 54(d)/EAJA cost-and-fee timing. This pack does **not** write
  `packs/links.json` (out of scope per the task); a follow-on composition pass
  would need to decide whether to also link the trial-verdict terminals to the
  CAFC pack, not just `judgment-entered`.
- **Bid-protest branch (Appendix C) was included** (task's optional item D) --
  it was sourceable from the local RCFC PDF's Appendix C text (pre-filing
  notice, sealed filing, initial status conference, TRO/PI practice,
  RCFC 52.1 administrative-record briefing). It is deliberately shallow (8
  nodes) relative to the patent track's depth, since the task scoped it as
  optional/small. `protest-sustained`'s $200,000 payoff is explicitly flagged
  as an illustrative bid-and-proposal-cost/re-compete proxy, not a share of
  the $1,000,000 baseline used elsewhere (bid-protest relief is
  injunctive/equitable, not usually a damages award on that scale).
- **Payoffs** are all explicitly flagged "PLACEHOLDER" / "override per matter"
  in their node `note`, parameterized off an illustrative $1,000,000 claim per
  the task's instruction: win terminals $1,000,000, ADR settlement $450,000
  (45%, illustrative discount, not rule-derived), all loss/dismissal terminals
  $0, bid-protest terminals use the smaller B&P-cost proxy described above.
- **28 U.S.C. §2412(d)(2)(G) "final judgment" timing subtlety**: EAJA's 30-day
  clock technically runs from when the judgment is no longer appealable (not
  bare RCFC 58 entry). The `costs-and-fees-window` node's note flags this
  explicitly rather than modeling a separate appeal-exhaustion gate before the
  EAJA/costs edges -- a deliberate simplification, not an oversight.
- **RCFC 14(b) contractor notice timing**: RCFC 14(b)(2)(B)(i) says a
  *plaintiff's* motion for notice must be filed at the time the complaint is
  filed, but RCFC Appendix J, Rule 3's cross-reference to "the United States
  has filed a motion pursuant to RCFC 14" implies the government/defendant
  can also bring this motion, on a later timeline (hence Appendix J's 49-vs-98
  day JPSR alternative). The pack models the fork as occurring around the
  patent early-meeting-of-counsel stage (after the answer), consistent with
  Appendix J's own timing anchor, and flags actor `examiner` for that edge --
  this is a reasonable reading of the two provisions together, not a directly
  quoted single rule, so it is noted here rather than left silent.
- **Vaccine Rules (Appendix B), tax-partnership (Appendix F), Indian Claims
  Commission (Appendix G), carrier cases (Appendix I), congressional
  reference (Appendix D), and military pay (Appendix K)** were all identified
  in the rules' table of contents but are out of the task's scope and not
  modeled.

## Validation

Ran a Python validation script (JSON parse; every edge `from`/`to` references
an existing node id; edge ids unique; `startNodeId` exists; every non-terminal
node has ≥1 out-edge and every terminal has 0; every non-start node reachable
from start; probability sums ≈1.0 on every node where ALL out-edges carry a
probability; probability never appears on an `applicant` edge; every terminal
carries `payoff` and `outcome`). Output:

```
nodes=68 edges=84 terminals=12
decision nodes=20 state nodes=36

NO ERRORS
```

## 10-line structure summary

1. Single connected graph, `startNodeId: claim-accrues`, 68 nodes / 84 edges / 12 terminals.
2. Threshold corridor (9 nodes): accrual → optional §286 patent-claim tolling → §2501 six-year bar → §1500 same-claim-pending bar → fork into standard-claim vs §1491(b) bid-protest filing.
3. Bid-protest branch (8 nodes, optional per task): pre-filing notice → sealed complaint → initial status conference → TRO/PI → administrative-record cross-motions → sustained/denied.
4. Pleading (11 nodes): RCFC 3 complaint ($405 fee) → clerk-on-AG service (RCFC 4, no plaintiff-service trap) → 60-day answer-or-12(b)-motion fork → answer → general/patent case-type fork.
5. General track (10 nodes): 49-day JPSR → scheduling order → ADR off-ramp → discovery hub with compel/protective-order loop → RCFC 56 summary judgment (30 days post-discovery) → trial or dispositive terminals.
6. Patent track (16 nodes, the densest branch): early meeting → RCFC 14(b) contractor-notice fork → 49/98-day JPSR → protective order → infringement contentions (56d) → invalidity contentions (+56d) → claim-term exchange (42d/28d) → joint claim construction chart (35d) → CC discovery close (28d) → CC hearing (7d/90d) → CC order → mandatory settlement discussion (14d/7d) → final contentions/expert discovery → dispositive-motion fork feeding back into the shared SJ/trial machinery.
7. Trial/post-trial (10 nodes, shared by both tracks): bench trial (no jury, ever) → findings/conclusions → RCFC 58 judgment → RCFC 59 motion window (28 days, unextendable) with new-trial/amend loops → RCFC 54(d) costs + EAJA fee window (30/30 days) → prevailing-party terminals.
8. 12 terminals carry `payoff`/`outcome`: 4 jurisdictional/pleading dismissals ($0), voluntary dismissal, 2 bid-protest outcomes, summary-judgment-for-government, `judgment-entered` (the exact-id CAFC composition anchor, reached via full plaintiff SJ), ADR settlement ($450k), and trial-verdict judgment-for-plaintiff/-government.
9. Biggest traps encoded: §1500's same-claim-pending bar (a purely self-inflicted jurisdictional loss, tagged `waiver-trap`); the RCFC 41(a)(1)(B) two-dismissal rule turning a second voluntary dismissal into an adjudication on the merits; RCFC 59's unextendable 28-day post-trial window (RCFC 6(b) bars any extension); and §1498(a)'s remedy ceiling (no injunction, no enhanced damages, no jury) that caps every patent-track win regardless of how strong the merits are.
10. All chance-node probabilities are flagged "TEACHING ESTIMATE" (not cited empirical CoFC statistics) and all terminal payoffs are flagged "PLACEHOLDER... override per matter," per the task's accuracy-over-invention instruction; no rule number or deadline in the pack is uncited to the RCFC/Appendix J/Appendix H/Appendix C PDFs or to verified statutory text.
