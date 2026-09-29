# What was wrong with the v1 engine, and what changed

v1 = `civ-pro-the-gathering/src/lib/graph` + `content/statecharts`. The port
reproduces v1 exactly when asked to (`crates/litgraph/tests/parity.rs`: value
iteration, per-edge Q, absorbing chain, Dijkstra, min cut, dominators, SCCs,
Pareto — all 6 packs), so every difference below is a deliberate, switchable
semantic change, not drift.

## Bugs (fixed)

| # | v1 behavior | consequence | fix |
|---|---|---|---|
| 1 | Q-values keyed `from→to` | 16 parallel edges across 5 packs collide; `absorbing-chain` picked policy edges by the *last-written* colliding Q | stable edge ids (`from->to#n` or authored `id`); Q is per edge |
| 2 | ties in Q broken by edge order | ITC `discovery-open` has a zero-cost self-loop ("fail to answer RFA") that ties the progress edge; v1 avoided a never-terminating policy only because of bug 1 | ties go to the edge nearer a terminal; policies are always proper |
| 3 | terminal payoff from label substrings (`issued`, `abandon`, `cancel` …) | ITC, FRCP, FRAP, FRCrimP, PTAB terminals mostly valued $0 → EV analyses on those packs were meaningless | authored `payoff` + `outcome` tags (v2); heuristic kept for v1 packs but flagged `payoff_source: heuristic` and warned on every response |
| 4 | value iteration and absorbing chain disagreed about mixed nodes (VI: you may "choose" the examiner's outcome; chain: world edges ignored) | the policy used in the chain was optimal for a different model than the one it was evaluated in | one explicit `mixed` mode for everything; default `nature-first` (authored interrupt probabilities fire, the chooser acts on the residual) |
| 5 | terminals with out-edges treated as absorbing | the whole post-grant region (IPR/PGR/EPR) invisible from filing | cross-pack links and v2 packs continue terminals with an explicit `accept` edge; v1 intra-pack stays absorbing (its payoffs assume it) and is warned |
| 6 | fee shifting = flat discount on any edge that can *structurally* reach a positive terminal | overstated recovery on long-shot lines | exact under the policy: `c·(1 − f·P(eligible terminal | edge target))`, iterated to a fixed policy |
| 7 | missing probabilities → uniform over *all* siblings, even authored ones | a node with one authored 0.9 edge and one unauthored edge became 50/50 | `residual` fill: authored kept, unauthored split what's left (`prob_fill: uniform` restores v1) |
| 8 | `days` = deadline length used as elapsed time | "expected days" was a sum of response windows, not a calendar forecast | separate `duration {min,mode,max}`; `elapsed` metric; triangular sampling in simulation |

## Bugs found by running the composed CoFC + Federal Circuit graph (fixed)

The first multi-forum analysis produced absurd answers; each traced to an
engine defect, now fixed with a regression test.

| # | symptom | cause | fix |
|---|---|---|---|
| 9 | §1498 plaintiff "optimally" dismissed on day one; `cofc::discovery-open` valued at −$1.27B | at a mixed node whose world edge had no probability, the fallback kept only our own edges — here a motion-to-compel self-loop — deleting the way out | **act-or-wait**: the chooser acts or waits (world edges fire); the default fallback. `act_or_wait_keeps_the_world_exit` |
| 10 | absorbing chain "singular" on packs where every state can terminate | transient set was graph-reachable, so an unvisited state with a self-loop poisoned I − Q | transient set = states the *policy* reaches; diagnostics name trapped states or bad rows |
| 11 | value iteration hit its cap silently | `converged: false` was never surfaced | `not-converged` warning naming the cyclic states |
| 12 | appeal remanded a CoFC case to the ITC (25% each) | the CAFC remand router can't know the origin forum | pack **instances** (`cafc@cofc`) in links.json: namespaced copies that remember how they were entered, with their own edge removals, perspective flips (`cafc@cofc-gov`), payoff transforms and edge patches (45-day rehearing when the US is a party) |
| 13 | `cafc-federal-circuit.json`'s `cert-not-sought`/`cert-denied` valued $0 regardless of whether the panel affirmed or reversed; entering rehearing or cert from a reversal silently discarded the win | those terminals sit downstream of `panel-decision` through a rehearing/mandate/cert region shared by every panel outcome, and a plain graph has no way for the same downstream node to remember which outcome got it there | **state flags**: `panel-to-affirmed`/`-reversed`/`-mixed` `sets` `panel-affirmed`/`panel-reversed`/`panel-mixed`; `rehearing-granted-to-decision` `clears` all three (a fresh disposition replaces the one being reheard); `cert-not-sought`/`cert-denied` carry `payoffByFlag` restoring the matching outcome's payoff. See `docs/PACK_SCHEMA.md#state-flags` and the *Markov on the node* entry below |

## Modeling limits (v1 had them; litgraph now names them)

- **Two-party stochastic game, not a one-player MDP.** Roles `self /
  opponent / nature` per pack, overridable per query (`perspective`).
  Opponents default to authored probabilities when present, else minimax.
  General-sum (opponent with its own payoffs) is not solved yet.
- **Markov on the node — rung 1 fixed.** Litigation has memory: estoppel
  after an IPR FWD, a waived Rule 12(h) defense, an RCE already filed, which
  way a Federal Circuit panel actually ruled before a rehearing/cert detour.
  A plain graph couldn't express "this edge exists only if X happened
  earlier" — every algorithm still keeps working unchanged, because *state
  flags* on edges (`sets`/`clears`/`requires`/`forbids`) are compiled into a
  product graph over `(node, flag-set)` before any algorithm runs; only
  `model::flags` knows flags exist. Modeled: IPR estoppel (35
  U.S.C. § 315(e), `ptab-patent-trial-appeal-board.json`), a waived personal
  jurisdiction/venue/process defense (FRCP 12(h)(1), `frcp-civil-procedure.json`),
  an RCE already filed (37 C.F.R. § 1.114, `mpep-prosecution.json`), and the
  CAFC win/loss-through-rehearing/cert bug (#13 above). See
  `docs/PACK_SCHEMA.md#state-flags`. Not solved by this: memory that spans
  *forums* needs an actual edge or link carrying the flag across packs (no
  automatic cross-pack propagation without one); per-claim/per-ground
  granularity (315(e) estoppel is modeled per proceeding, not per claim);
  arbitrary-depth counters (the RCE example is a bounded "has this happened
  before" flag, not a true count — an N-tier counter needs N flags and is
  bounded by `max_product_nodes`, not free); and belief-state memory
  (information value, POMDPs) remains unsolved, same as before.
- **Payoff scale across packs.** Composed graphs mix v1 heuristic payoffs
  (scoring points × $1k) with v2 placeholders ($1M claim). Always set
  `scenario.payoffs` for the matter at hand; responses warn when they are
  not authored.
- **Risk.** Mean-optimal by default. CARA is exact; CVaR/percentiles are
  reported from simulation but not optimized.
- **Probabilities are teaching estimates** in most packs (vintages noted
  in node/edge notes). `tornado` shows which ones the answer actually
  depends on — calibrate those first.
- **Durations** are sparse; `elapsed` falls back to deadline windows and
  every response says so.

## Architecture changes

- One compiled, namespaced graph for any set of packs (`frcp::…`,
  `cafc::…`) joined by `packs/links.json`, instead of six islands each ending
  in its own copy of "Federal Circuit affirms".
- Every modeling choice that v1 hard-coded (mixed nodes, probability fill,
  opponent model, objective, perspective, fee shifting, discounting) is a
  scenario field with a default, visible in `provenance.modes`.
- Custom functions everywhere a number is consumed (see COST_FUNCTIONS.md).
- Solver runs per SCC in reverse topological order: acyclic regions get one
  exact backup, cycles iterate locally. Exact-er and orders of magnitude
  faster than whole-graph sweeps; the chain is solved by LU with a diagnostic
  when a policy never terminates.
- One JSON request → one JSON response with `warnings` and `provenance`.

## Content gaps found while porting

- FRAP, FRCrimP, PTAB: no hours/fees → dollar analyses are all zeros.
- 0 of 73 v1 terminals carry an authored payoff.
- Actor vocabulary `applicant/examiner/office/either` is patent-prosecution
  language reused for courts; v2 `roles` maps it explicitly.
- Deadlines are calendar-day counts; court-day and holiday rules (FRCP 6,
  RCFC 6) live in civ-pro's `src/engine/clock` and are not yet ported.
