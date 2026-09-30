# Cost, weight, edge and utility functions

Every number an algorithm consumes is produced by an **expression** evaluated
on an edge or a terminal. Built-ins are just named expressions (`litgraph
describe` prints their source), so there is one mechanism, not two: anything
below marked *expression* works today without recompiling.

## Where custom functions plug in

| hook | evaluated on | used by | scenario field / op arg |
|---|---|---|---|
| cost | edge | solve, chain, simulate, explain, sweep, tornado | `scenario.cost` (name or inline expr) |
| utility | terminal | solve, chain, simulate | `scenario.utility` |
| path weight | edge | `path` | `op.metric` |
| Pareto objectives | edge (N of them) | `pareto` | `op.objectives` |
| reported totals | edge | `path`, `chain`, `simulate` | `op.report`, `op.metrics` |
| mask (counterfactual) | edge | everything | `scenario.mask` |
| probability transform | world edge | everything | `scenario.probability_fn` |
| cut capacity | edge | `structure what=mincut` | `op.capacity` |
| fee-shift eligibility | terminal | solve, simulate | `scenario.fee_shift.eligible` |
| named library | — | — | `scenario.metrics`, `scenario.utilities` (shadow built-ins) |
| parameters | — | all expressions | `scenario.params` (any name; sweepable) |

Variables and functions available inside each: `litgraph describe`
(`edge_variables`, `edge_functions`, `terminal_variables`, `math_functions`).
Test any expression with `{"op":"metric","spec":"..."}` before using it.

## The catalog

Status: **built-in** = named metric/utility shipped; **expression** = one line
with existing variables; **data** = expressible once packs carry the `attrs`;
**engine** = needs new engine capability (tracked).

### A. Resource cost (additive along a path)

| function | expression | status |
|---|---|---|
| attorney labor + fees (v1 dollarCost) | `hours * rate + fees` | built-in `dollars` |
| hours only / fees only | `hours`, `fees` | built-in |
| only our spend | `is_self * (hours * rate + fees)` | built-in `self_dollars` |
| blended team rate | `hours * (0.3 * partner_rate + 0.7 * associate_rate) + fees` | expression |
| expert / vendor / e-discovery cost | `hours * rate + fees + attr('expert_cost', 0) + attr('vendor_cost', 0)` | data |
| client-side burden (exec time, custodians) | `attr('client_hours', 0) * client_rate` | data |
| bond / security capital cost | `attr('bond', 0) * cost_of_capital * elapsed / 365` | data |
| contingency-fee economics (counsel's view) | cost `hours * shadow_rate`, utility `contingency * payoff` | expression |

### B. Time

| function | expression | status |
|---|---|---|
| deadline window (v1 `days`) | `days` | built-in |
| expected elapsed calendar time | `elapsed` (duration.mode → deadline → 0) | built-in |
| stochastic duration | triangular(min, mode, max) sampled in `simulate` | built-in |
| delay carrying cost | `hours * rate + fees + elapsed * carry_per_day` | built-in `time_value` |
| discounting | `scenario.discount_annual` (γ = (1+r)^(−elapsed/365) per edge) | built-in |
| hard-deadline exposure | `has_deadline * (1 - extendable)` | built-in `hard_deadlines` |
| court-day vs calendar-day computation, holidays, FRCP 6 | — | engine (port civ-pro `src/engine/clock`) |

### C. Risk and likelihood

| function | expression / mechanism | status |
|---|---|---|
| most likely line | `-ln(p)` summed = −ln P(path) | built-in `surprise` |
| path probability | reported on every path | built-in |
| cost × likelihood 3-way frontier | `pareto objectives=[dollars, elapsed, surprise]` | built-in (default) |
| risk aversion (exact) | `objective: {type: cara, a}` | built-in |
| loss aversion | `payoff < 0 ? loss_aversion * payoff : payoff` | built-in `loss_averse` |
| worst case / robust | `objective: {type: worst}` | built-in |
| downside tail (CVaR), P(loss), percentiles | `simulate` | built-in |
| waiver / trap exposure | `tag('waiver-trap') + valence_bad` | built-in `traps` |
| chokepoints weighted by likelihood or money | `structure mincut capacity=p` / `=dollars` | built-in |
| judge / forum / examiner calibration | `probability_fn: "label_has('grant') ? p * judge_grant_mult : p"` | expression |
| uncertain probabilities (robust to the credible range) | `objective: {type: robust, credibility}` (rectangular L1 robust MDP over Dirichlet credible sets; see docs/UNCERTAINTY.md) | built-in |
| judge-specific evidence ("granted 3 of 4") | `scenario.observe` (conjugate Dirichlet update; every op uses the posterior mean) | built-in |
| optimizing CVaR (not just reporting it) | `objective: {type: cvar, alpha}` (Rockafellar–Uryasev; see docs/CRITIQUE.md) | built-in |

### D. Outcome value (terminal)

| function | expression / mechanism | status |
|---|---|---|
| risk-neutral payoff | `payoff * stakes` | built-in `ev` |
| per-matter payoff | `scenario.payoffs` | built-in |
| outcome-class weighting | `tag('win') * payoff`, `tag('settlement') * payoff * 0.9` | expression |
| fee shifting (§ 285, EAJA, 1927, Rule 37, contract) | `fee_shift: {fraction, eligible: "tag('fee-eligible')"}`, exact under policy via policy iteration | built-in |
| Rule 68 offer-of-judgment cost shift | `settle` op's `rule68: {offer, costs, eligible?}` (FRCP/RCFC 68(d); see docs/SETTLEMENT.md); or `fee_shift` with `eligible: "payoff < offer"` and a costs-only fraction | built-in |
| prejudgment interest, damages that grow with time | `payoff * (1+r)^(elapsed_total/365)` (see "Path-dependent terminal variables" below) | expression (exact in `simulate`; `solve`/`chain` see `elapsed_total` as 0) |
| collectability / judgment-proof defendant | `payoff * collect_p` | expression |
| non-monetary objectives (injunction, precedent, deterrence) | terminal `attrs` + utility expr, e.g. `payoff + node.precedent_value * precedent_weight` | data |

### E. Strategic / adversarial

| function | expression / mechanism | status |
|---|---|---|
| burden imposed on the opponent | `is_opponent * (hours * opp_rate + fees)` | built-in `opponent_dollars` |
| leverage (their cost vs ours) | `pareto objectives=[self_dollars, -opponent...]` → use `[self_dollars, opp_slack]` where `opp_slack = big - opponent_dollars` | expression (ratio of sums is not additive; frontier is the honest form) |
| adversarial opponent | `opponent: adversarial` (minimax) | built-in |
| perspective flip (defendant's view) | `perspective: {applicant: opponent, examiner: self}` + negated payoffs | built-in |
| settlement range and price (ZOPA, Nash, Rubinstein, midpoint) | `settle` op with `opponent_objective` (docs/SETTLEMENT.md) | built-in |
| settlement timing | `settle`'s `timing` (surplus along the likely line, optimal stopping with settling always available); or `compare` with `policy` forcing settle at different nodes | built-in |
| option value (keeping more terminals reachable) | betweenness / reachable-terminal count | partial (structure ops); engine for a per-edge metric |
| estoppel / waiver that persists within a forum (IPR § 315(e), FRCP 12(h)) | edge `sets`/`clears`/`requires`/`forbids` a flag; `flag("x")` in a cost/utility expression | built-in (`docs/PACK_SCHEMA.md#state-flags`) |
| estoppel / preclusion that persists *across forums* | flags set in one pack, read by a `flag()` expression or `forbids` on an edge in another pack (or a `links.json` link edge) — same mechanism, wired across the composed graph | built-in for a link edge in the flag's own graph; a pack whose only connection to the flag-setting pack is a scenario, not a compiled edge, still needs a shared graph to see it |
| information value (discovery changes p) | `voi` op: EVPI per chance node's outcome and probabilities, EVSI of a study worth `k` observations vs its cost (Dirichlet beliefs; see docs/UNCERTAINTY.md). Learning *during* the process (a full POMDP) is still engine | built-in |
| non-zero-sum opponent with its own payoffs | `opponent_objective: terminal-expr` (subgame-perfect equilibrium by backward induction; see docs/CRITIQUE.md) | built-in |

### F. Conduct and sanctions

| function | expression | status |
|---|---|---|
| price sanctions exposure out | `tag('sanctions') ? 1e9 : hours * rate + fees` | expression |
| Rule 11 / 1927 / inherent-power risk | `attr('sanction_p', 0) * attr('sanction_usd', 0) + hours * rate + fees` | data |

## Path-dependent terminal variables

A terminal (utility) expression can read three variables describing the path
that reached it, in addition to the ordinary terminal variables (`payoff`,
`node.<attr>`, params, ...; see `litgraph describe`'s `terminal_variables`):

| variable | meaning |
|---|---|
| `spent` | cumulative cost (the scenario's `cost` metric) along the path to this terminal |
| `elapsed_total` | cumulative elapsed days along the path (sampled durations if `simulate`'s `sample_durations` is on) |
| `steps` | number of edges traversed to reach this terminal |

These support prejudgment interest and time-growing damages, e.g.:

```jsonc
"utility": "payoff * (1 + r) ^ (elapsed_total / 365)"   // compounds on elapsed calendar time
"utility": "payoff - 500 * steps"                        // a flat per-step (e.g. per-hearing) drag
```

**Exactness.** A terminal is Markov on the *node* in `solve` (SCC-ordered
Bellman backups) and `chain` (the absorbing chain): both value a node once,
independent of how it was reached. But the same terminal can be reached
having spent different amounts on different paths (a converging graph, or a
cycle that adds cost each time around), so there is no single correct
`spent`/`elapsed_total`/`steps` to hand those algorithms — they use `0` for
all three (`(1+r)^0 == 1`: no interest is applied) and `litgraph
validate`/every op that resolves a scenario reports a `path-variable-in-markov`
warning when the utility (or a `fee_shift.eligible` expression, which is
always Markov) references one of them. Only
[`simulate`](../crates/litgraph/src/algo/sim.rs) knows the actual sampled
trajectory and evaluates the utility expression per run with the real
accumulated values — use `simulate`, not `solve`/`chain`, whenever the
answer should reflect exact prejudgment interest or time-growing damages.
See `examples/cofc-prejudgment-interest-sim.json`.

## Writing new ones

1. Try it: `litgraph metric --arg spec="hours * rate * 1.2 + fees"`.
2. Name it in the scenario (`"metrics": {"biglaw": "..."}`) and use the name.
3. If it needs data the packs lack, add an `attrs` key to the edges (schema v2)
   and default it with `attr('x', 0)` so older packs still evaluate.
4. If it is broadly useful, add it to `METRICS`/`UTILITIES` in
   `crates/litgraph/src/metrics.rs` — it then shows up in `describe`.
