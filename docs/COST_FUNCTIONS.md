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
| optimizing CVaR (not just reporting it) | — | engine |

### D. Outcome value (terminal)

| function | expression / mechanism | status |
|---|---|---|
| risk-neutral payoff | `payoff * stakes` | built-in `ev` |
| per-matter payoff | `scenario.payoffs` | built-in |
| outcome-class weighting | `tag('win') * payoff`, `tag('settlement') * payoff * 0.9` | expression |
| fee shifting (§ 285, EAJA, 1927, Rule 37, contract) | `fee_shift: {fraction, eligible: "tag('fee-eligible')"}`, exact under policy via policy iteration | built-in |
| Rule 68 offer-of-judgment cost shift | `fee_shift` with `eligible: "payoff < offer"` and costs-only fraction | expression |
| prejudgment interest, damages that grow with time | needs elapsed-so-far at the terminal (path-dependent) | engine (simulate can; MDP needs state augmentation) |
| collectability / judgment-proof defendant | `payoff * collect_p` | expression |
| non-monetary objectives (injunction, precedent, deterrence) | terminal `attrs` + utility expr, e.g. `payoff + node.precedent_value * precedent_weight` | data |

### E. Strategic / adversarial

| function | expression / mechanism | status |
|---|---|---|
| burden imposed on the opponent | `is_opponent * (hours * opp_rate + fees)` | built-in `opponent_dollars` |
| leverage (their cost vs ours) | `pareto objectives=[self_dollars, -opponent...]` → use `[self_dollars, opp_slack]` where `opp_slack = big - opponent_dollars` | expression (ratio of sums is not additive; frontier is the honest form) |
| adversarial opponent | `opponent: adversarial` (minimax) | built-in |
| perspective flip (defendant's view) | `perspective: {applicant: opponent, examiner: self}` + negated payoffs | built-in |
| settlement timing | `compare` with `policy` forcing settle at different nodes | built-in |
| option value (keeping more terminals reachable) | betweenness / reachable-terminal count | partial (structure ops); engine for a per-edge metric |
| estoppel / waiver that persists across forums (IPR § 315(e), claim preclusion) | needs history-dependent state | engine (state flags → product graph) |
| information value (discovery changes p) | needs belief state (POMDP) | engine |
| non-zero-sum opponent with its own payoffs | general-sum equilibrium | engine |

### F. Conduct and sanctions

| function | expression | status |
|---|---|---|
| price sanctions exposure out | `tag('sanctions') ? 1e9 : hours * rate + fees` | expression |
| Rule 11 / 1927 / inherent-power risk | `attr('sanction_p', 0) * attr('sanction_usd', 0) + hours * rate + fees` | data |

## Writing new ones

1. Try it: `litgraph metric --arg spec="hours * rate * 1.2 + fees"`.
2. Name it in the scenario (`"metrics": {"biglaw": "..."}`) and use the name.
3. If it needs data the packs lack, add an `attrs` key to the edges (schema v2)
   and default it with `attr('x', 0)` so older packs still evaluate.
4. If it is broadly useful, add it to `METRICS`/`UTILITIES` in
   `crates/litgraph/src/metrics.rs` — it then shows up in `describe`.
