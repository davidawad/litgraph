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
  General-sum (opponent with its own payoffs) is solved by backward
  induction on the SCC DAG when `scenario.opponent_objective` is set — see
  "General-sum opponents" below, including `explain`'s per-option `regret`,
  which is computed from the mover's own criterion (their `opp_q` at a
  general-sum opponent's node, not an assumed adversary).
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
  within a trajectory (a POMDP that learns as the case proceeds) remains
  unsolved. Information value *before* committing to a policy is now
  priced by the `voi` op (see "Uncertain probabilities" below).
- **Payoff scale across packs.** Composed graphs mix v1 heuristic payoffs
  (scoring points × $1k) with v2 placeholders ($1M claim). Always set
  `scenario.payoffs` for the matter at hand; responses warn when they are
  not authored.
- **Risk.** Mean-optimal by default. CARA is exact. CVaR/percentiles are
  reported from simulation (`simulate`'s `cvar`/percentile fields) and, since
  `objective: {type: cvar, alpha}` (see "CVaR-optimal policies" below), can
  also be *optimized*, with documented grid-discretization exactness limits.
- **Probabilities are teaching estimates** in most packs (vintages noted
  in node/edge notes). `tornado` shows which ones the answer actually
  depends on — calibrate those first. Since the uncertainty layer
  (`docs/UNCERTAINTY.md`), each chance node also carries a Dirichlet
  belief (strength from a calibration entry's `n`, else a warned default):
  `scenario.observe` updates it, `objective: robust` optimizes against its
  credible set, and `posterior`/`voi` propagate it and price information.
- **Durations** are sparse; `elapsed` falls back to deadline windows and
  every response says so.

## CVaR-optimal policies

`objective: {type: cvar, alpha, grid?, y_lo?, y_hi?}` maximizes `CVaR_alpha`
of the total outcome (mean of the worst `alpha` fraction) — not just
reports it from a simulation the way `simulate`'s `cvar`/`p_loss`/percentile
fields do.

**Method.** Rockafellar & Uryasev (2000)'s variational form,

```text
CVaR_alpha(X) = max_ζ [ ζ − (1/alpha)·E[(ζ − X)⁺] ]
```

is concave in the Value-at-Risk threshold `ζ`. Bäuerle & Ott (2011) show
that for a Markov (or, as here, an SCC-ordered stochastic-game) total
reward, the inner expectation becomes an ordinary backward induction once
the state is augmented with `y`, the "remaining budget" `ζ` minus the value
accumulated so far: taking an edge shifts `y ↦ y + cost(edge)`, and at a
terminal the local objective `−(1/alpha)·max(y − utility(terminal), 0)`
depends on `y` alone. `crates/litgraph/src/algo/cvar.rs` runs one SCC-ordered
backward-induction pass (the same `scc`/`is_cyclic` decomposition as
`mdp::solve`) computing `W(node, y)` on a discretized grid of `y`; because
the recursion doesn't reference `ζ` except at the boundary, every candidate
`ζ` is answered by the *same* pass — the outer maximization is a lookup
`max_j [ ys[j] + W(start, ys[j]) ]` at the start node, not a repeated solve.

**Exactness limits.**

- **Grid discretization.** `y` is discretized to `grid` points (default 41)
  spanning a conservative default range (the terminal-utility range widened
  by the graph's total absolute edge cost); off-grid lookups use linear
  interpolation. Error is bounded by the grid step times the local slope of
  the (piecewise-linear, slope ≤ `1/alpha`) value function, and is *worse*
  near a kink the grid doesn't land on exactly (a plateau-then-cliff value
  function, as at a deterministic terminal, can undershoot the true optimum
  by close to a full grid step — see `tests/cvar.rs`'s hand-checked case).
  Increase `grid` for a tighter answer, or override the default `y` range
  directly with `y_lo`/`y_hi` (set together) for a graph whose default is a
  poor fit (e.g. very costly cycles) or unnecessarily wide.
- **Cyclic components** are iterated to a fixed point exactly like
  `mdp::solve`'s cyclic handling (same convergence tolerance/cap,
  `Solution.converged`/`unconverged`), on the whole `y`-row per node per
  iteration.
- **Doesn't compose with `discount_annual`, `fee_shift`, or a general-sum
  `opponent_objective`** (each triggers a `cvar-ignores-*` warning):
  discounting a per-edge value while additively shifting a "remaining
  budget" state are two different notions of time value that would need a
  more careful joint augmentation; fee-shift's cost depends on the (here,
  budget-dependent) policy through policy iteration, which isn't combined
  with the grid solve; and jointly optimizing our CVaR against a
  self-interested opponent's own equilibrium is a substantially harder
  problem (not attempted here — the opponent is modeled adversarially, as
  in ordinary `solve`).
- **Reported policy is a single canonical rollout**, not the true (budget-
  dependent) optimal policy at every `(node, y)`: the CVaR-optimal policy
  in general chooses differently depending on how much budget remains, but
  `Solution.choice`/`value` (as consumed by `chain`/`simulate`/`explain`)
  are keyed by node alone. `cvar::solve` walks the single trajectory from
  `(start, ζ*)` and records the first-visit choice per node; a node
  revisited with a different remaining budget (a cycle) keeps its
  first-visit choice, which need not be optimal for that later visit.
  `Solution.q` is left `NaN` for the same reason: no single per-edge number
  is correct independent of the budget it's evaluated at.
- **Verification.** `tests/cvar.rs` compares `cvar::solve` against brute-force
  enumeration of every deterministic memoryless policy (exact distribution
  enumeration + the same `CVaR_alpha` formula) on small acyclic graphs,
  including a randomized proptest sweep, within a grid-step-scaled tolerance.

## Uncertain probabilities

Probabilities are beliefs, not constants: a Dirichlet per chance draw,
updated by `scenario.observe`, optimized against by `objective: robust`
(rectangular L1 robust MDP over Bayesian credible sets: Iyengar 2005,
Nilim & El Ghaoui 2005, Petrik & Russel 2019), propagated by `posterior`,
and priced by `voi` (EVPI/EVPPI/EVSI as expected regret). Method, citations
and exactness limits (per-node credibility, cyclic nodes, risk-neutral
valuation, public information) are in `docs/UNCERTAINTY.md`.

## General-sum opponents

`scenario.opponent_objective` (a terminal expression, e.g. `"node.opp_fees +
node.stake"`) switches the opponent from a zero-sum adversary (minimizing
our value) to a self-interested player maximizing *their own* payoff.

**Method.** The compiled graph is an extensive-form, perfect-information
game: exactly one party moves at each node. Backward induction over such a
game computes the subgame-perfect equilibrium directly — `algo/equilibrium.rs`
runs the same SCC-ordered pass as `mdp::solve`, but tracks *two* value
vectors (`self`, `opponent`); at an opponent-controlled node the opponent
picks the edge maximizing their own continuation value
(`-opponent_dollars(edge) + opponent_objective(to)`, the `opponent_dollars`
built-in giving their per-edge cost symmetrically to how `cost`/`utility`
give ours), and *both* players' values propagate along whichever edge that
turns out to be. `opponent_objective: None` (the default) delegates to
`mdp::solve` unchanged — the zero-sum special case, reproduced exactly
(`tests/general_sum.rs`'s `zero_sum_opponent_objective_none_reproduces_mdp_solve_exactly`).

**Exactness limits.**

- **Cyclic components don't have a convergence guarantee.** A single-agent
  MDP's value iteration is a contraction; general-sum best-response
  iteration (each side re-optimizing against the other's last-iteration
  value) is not guaranteed to converge for arbitrary payoffs — it can
  oscillate. `equilibrium::resolve` iterates jointly to the same tolerance/
  cap as `mdp::solve` and reports `converged: false` / `unconverged` exactly
  as honestly, rather than returning a number that looks confident.
- **`Objective::Cara`/`Worst` are honored for `self`, never for the
  opponent.** `self`'s risk objective applies to *self*'s aggregation over
  nature's draws (world edges, interrupts, `WAIT`) — the same
  `mdp::aggregate` helper `mdp::solve` uses, shared so the two can't drift —
  both when *choosing* whether to wait (`Ctx::best_option`'s `crit_wait`)
  and in the final value tally (`Ctx::backup`); a general-sum opponent's own
  aggregation always stays plain expectation, since `opponent_objective`
  gives them an objective function, not a modeled risk preference.
  `Objective::Cvar` still doesn't compose with `opponent_objective` (a
  `cvar-ignores-opponent-objective` warning): jointly optimizing our CVaR
  against a self-interested opponent's own equilibrium is a substantially
  harder problem, not attempted here.
- **`explain`'s per-option `regret`** is computed from the mover's own
  criterion: `self_q` (with `NodePlan::minimize`'s adversarial sign flip) at
  every node except a general-sum opponent's, where it uses their own
  `opp_q` instead (`EquilibriumSolution::opponent_q`, `NaN` for inactive/
  terminal-sourced edges like `solution.q`) — `regret = opp_q(best) -
  opp_q(option)`, always `>= 0` at the actual choice. `NodePlan::minimize`
  stays `true` at every `Control::Opponent` node regardless of
  `opponent_objective` (the general-sum solver doesn't consult it at all,
  by design — see `best_option`'s doc comment), so `explain` — not the
  solver — is what has to branch on `opponent_q.is_some()`.
- **Not combined with `fee_shift`** (a warning is reported): fee-shift's
  policy-iteration cost adjustment is defined in terms of `self`'s policy
  only.

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

## Gaps found by replaying famous cases

`tests/cases/` replays ten real procedural histories through the packs
(`tests/README.md`). Each gap below is recorded in a case file as an
expected failure, and the test fails once a pack closes it.

- **No Supreme Court merits stage.** Every `cert-granted` is terminal, so
  a reversal, an affirmance, or a grant-vacate-remand (Hughes Aircraft,
  1997) cannot send a case back to the court below. This affects every
  case that reached the Court.
- **No interlocutory appeal out of the district court.** `links.json`
  joins FRCP to the courts of appeals only at `notice-of-appeal-filed`
  (a final judgment). A collateral-order appeal from the denial of
  qualified immunity (Iqbal) can't cross, though FRAP's own interlocutory
  door exists.
- **No rehearing or certiorari after a CAFC vacate-and-remand.**
  `panel-to-remand` goes straight to a `remanded-to-*` terminal (Arthrex,
  2019-20: en banc denied, then cert granted).
- **PTAB deadlines have no rule set.** The PTAB pack's `forum: "ptab"`
  maps to no clock rule set (37 C.F.R. § 1.7 / § 90.3(c) are not
  implemented), so the `deadlines` op can't compute any PTAB-side
  deadline. The CAFC instance's copy of the 63-day notice-of-appeal window
  can be computed.
- **Anachronistic forced steps.** The PTAB pack forces a patent-owner
  sur-reply and Board preliminary guidance on a motion to amend (2018 and
  2019 practice), and has no partial institution (pre-SAS practice). The
  CAFC pack forces a bill of costs before the certiorari window.

Fixed while writing the cases: the FRCP dismissal and summary-judgment
terminals had no route to an appeal; FRAP had no "no transcript" or "no
stay" branch; the 45-day U.S.-party rehearing window was missing from
`rehearing-time-expires` and from the whole `cafc@itc` instance; and the
holiday calendar applied Juneteenth and MLK Day to years before they
existed.
