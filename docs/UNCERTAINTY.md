# Uncertain probabilities: Bayesian updates, robust solve, value of information

Most probabilities in the packs are teaching estimates; the calibrated
ones (`docs/CALIBRATION.md`) are population rates. Neither is *this
matter's* probability. This layer treats every chance probability as
uncertain, lets observed outcomes update it, picks strategies that hold up
across the uncertainty, and prices information: what learning something
before committing would be worth.

| what | where | answers |
|---|---|---|
| Dirichlet belief per chance node | `scenario.uncertainty` | how sure are we of each node's probabilities? |
| Bayesian update | `scenario.observe` | this judge granted 3 of 4 similar motions: what now? |
| robust solve | `objective: {type: robust}` | which strategy survives the plausible range of probabilities? |
| posterior propagation | `{"op": "posterior"}` | credible interval on the case value and on each option; P(each option is optimal) |
| value of information | `{"op": "voi"}` | what is it worth to learn a node's outcome or its rate, or to buy a study worth `k` observations? |

## The belief model

Every node whose plan draws from a distribution gets a Dirichlet
(`crates/litgraph/src/scenario/belief.rs`). These nodes are:

- a **chance node**: its draws;
- a **nature-first interrupt**: the interrupt edges plus a residual slot,
  "no interrupt, the chooser acts";
- an **act-or-wait** node: the world edges `WAIT` lets fire.

The prior mean is the probability vector the scenario already resolved: the
authored value, calibration, `probabilities`, `probability_fn`, fills.
The prior strength is a pseudo-count total `c`, the *concentration*:
`α = c · p`. `c` comes from, in order:

1. `scenario.uncertainty.concentration: {node ref: c}`, the matter's own
   judgment;
2. the `n` of a calibration entry that set one of the node's edge
   probabilities (the smallest if several). For example, `ptab-fy2024`'s
   institution rate carries `n = 1087` decisions;
3. `scenario.uncertainty.default_concentration`, else **10**. This is an
   **estimate** ("as if the authored probability had been seen in ten
   comparable matters"). Every op that depends on it emits a
   `prior-concentration-estimated` warning at that node.

A population `n` treats this matter as exchangeable with the population.
When the judge, the art unit, or the claim type is atypical, that
overstates certainty; set a lower `concentration` for the node. A
component resolved to probability 0 (e.g. the sibling of a
`scenario.facts` branch) has `α = 0`: it is impossible in every draw. A
node left with fewer than two possible outcomes is known, not uncertain.

## Bayesian updating: `scenario.observe`

```json
"observe": { "cofc::sj-ruling": { "cofc::e-sj-govt": 3, "cofc::e-sj-denied": 1 } }
```

This is the Dirichlet–multinomial conjugate update, `α' = α + counts`
(Gelman et al., *Bayesian Data Analysis*, 3rd ed., §3.4; Beta is the
two-outcome case). The view then uses the posterior mean
`α' / Σα'` in place of the prior mean, so **every op** (solve, chain,
simulate, explain, sweep, ...) runs on the updated probabilities.
`provenance.modes.observe` records what was observed. An observation
applies to every state-flag copy of the edge, the same way `facts` and
calibration do. Observing a choice edge, a negative count, or an unknown
ref is an error.

Worked closed form (`tests/uncertainty.rs`): prior `p = 0.6`, `c = 10` gives
Beta(6, 4). Observing 3 granted of 4 gives Beta(9, 5), so the probability
used is 9/14 ≈ 0.643. With `n = 1087` from the PTAB calibration, four more
grants barely move 0.681.

## Robust solve: `objective: {type: robust, credibility?, radius?, samples?, seed?}`

This maximizes the **worst-case** expected value over a credible set of
probabilities, using the rectangular robust MDP of Iyengar (2005), "Robust
dynamic programming", *Mathematics of Operations Research* 30(2):257–280,
and Nilim & El Ghaoui (2005), "Robust control of Markov decision processes
with uncertain transition matrices", *Operations Research* 53(5):780–798.
At each chance draw, nature picks the distribution independently per node
("rectangular") from an ambiguity set, and the ordinary SCC-ordered Bellman
backup takes the minimum:

```text
V(n) = min_{p ∈ P_n} Σ_i p_i · q_i,   P_n = { p ∈ Δ(support) : ‖p − θ̄_n‖₁ ≤ ψ_n }
```

- **Ambiguity set.** This is an L1 ball around the posterior mean,
  restricted to the posterior's support. Its radius `ψ_n` is the
  `credibility` quantile (default 0.9) of `‖θ − θ̄_n‖₁` under the node's
  Dirichlet posterior, estimated from `samples` (default 2000) seeded
  draws. This is the Bayesian credible ambiguity set of Petrik & Russel
  (2019), "Beyond confidence regions: tight Bayesian ambiguity sets for
  robust MDPs", *NeurIPS* 32
  ([proceedings](https://proceedings.neurips.cc/paper/2019/hash/b994697479c5716eda77e8e9713e5f0f-Abstract.html)).
  Setting `radius` fixes the same L1 radius everywhere instead: `0` is the
  nominal solve, and `2` is the worst case over the support, which equals
  `objective: worst`.
- **Inner problem.** The minimization has a closed form: move `ψ/2` of
  mass onto the worst outcome, taking it from the best outcomes first
  (`algo/robust.rs::worst_l1`).
- **Report.** `solve` adds a `robust` block: `nominal_value` (expected
  value at the posterior mean), `robust_value`, `price_of_robustness`,
  `ambiguous_nodes`, `max_radius`, and `policy_changes`, which lists every
  reachable decision of ours where the robust choice differs from the
  nominal one. `chain`, `simulate` and `explain` run the robust *policy*
  under the posterior-mean probabilities.

**Limits.** Credibility is per node. The probability that *every* node's
true distribution lies in its set is lower (union bound), so the robust
value is a conservative node-level bound, not a joint credible bound. Only
nature's draws are ambiguous; opponent choices follow `opponent` mode. The
robust objective doesn't compose with a general-sum `opponent_objective`
(warned as `robust-ignores-opponent-objective`; the equilibrium solve uses
the posterior mean). A KL ambiguity set is not implemented; L1 is the one
with the exact greedy inner step.

## Posterior propagation: `{"op": "posterior", from?, node?, samples?, seed?, credibility?}`

Each draw samples every uncertain node's `θ ~ Dirichlet(α)` independently
and re-solves the game risk-neutrally (`algo/posterior.rs`). The result
has:

- `value`: `{nominal, mean, lo, hi}` at the start. `nominal` is `solve`'s
  number; `lo`/`hi` bound the central `credibility` interval.
- `options`: each option at `node` (default: the start), including `WAIT`
  at an act-or-wait node. Each option has its Q interval, `p_optimal` (the
  share of draws in which it is the mover's choice), and `nominal_best`.

`mean` generally exceeds `nominal` wherever a decision can adapt, because
`E[max] ≥ max E`. The gap is the value of perfect information about every
probability at once (`voi`'s `evpi_total`, up to sampling error, where the
linearity argument below holds).

## Value of information: `{"op": "voi", from?, samples?, seed?, top?, max_nodes?, studies?}`

This is classical preposterior analysis (Raiffa & Schlaifer 1961, *Applied
Statistical Decision Theory*; Howard 1966, "Information value theory",
*IEEE Transactions on Systems Science and Cybernetics* 2(1):22–26),
implemented in `algo/voi.rs`. Let `π̄` be our policy at the posterior mean.
Each quantity is the expected **regret** of having committed to `π̄`:

| field | definition | computed |
|---|---|---|
| `evpi_outcome` | learn which outcome this node will produce | exactly, one solve pair per outcome |
| `evppi` | learn this node's probabilities | Monte Carlo over its posterior |
| `evpi_total` | learn every probability | Monte Carlo over the joint posterior |
| `studies[].evsi` | a study worth `k` observations of the node (expert report, mock panel, survey of this judge) | Monte Carlo: θ, then `k` outcomes, then the updated posterior mean |

Writing each as a regret makes every term `≥ 0`, and **exactly 0** when no
possible answer changes the decision. The regret form equals the textbook
`E[max] − max E` for a node visited at most once per trajectory: the start
value under a fixed policy is linear in that node's probabilities, and the
posterior mean is a martingale. For such nodes,
`evpi_outcome ≥ evppi ≥ evsi(k) ≥ 0`, and `evsi(k) → evppi` as `k` grows.
The Monte Carlo estimates carry `std_error`.

Hand-checked case (`tests/uncertainty.rs`): a sure $50 against a ruling
worth $100 with θ ~ Beta(1, 1) gives `evpi_outcome` = 25,
`evppi` = E[max(0, 100θ − 50)] = 12.5, and `evsi(k = 1)` = 25/3.

**Studies vs. cost.** Each study gives `cost` (in the scenario's cost
units) or `cost_edge` (an edge whose scenario cost metric is the price,
e.g. the pack's "retain expert" step). The row reports `net = evsi − cost`
and `worth_paying`. `evpi_outcome` is an upper bound: no study of that node
is worth more.

**Ranking.** Uncertain nodes reachable from the start are screened, up to
`max_nodes` (`nodes_truncated` says if more exist). Each gets
`evpi_outcome`. The `top` rows by that screen also get the Monte Carlo
`evppi`. Rows are ranked by `evppi` where computed, then by
`evpi_outcome`.

**Limits.**

- **Nodes on a cycle** (e.g. a 12(b) ruling that can grant leave to amend
  and come back) report `evpi_outcome: null`. Forcing one outcome on every
  visit loops forever and is not clairvoyance about a single draw. Such
  nodes are always screened in for `evppi`.
- Where a node is revisited, or an adversarial opponent re-optimizes, the
  linearity argument fails. The numbers are then the regret definition, not
  an exact `E[max] − max E`.
- Information is modeled as **public**: the opponent's modeled response is
  re-solved along with ours.
- `unconverged_solves` counts solves that hit an iteration cap.

**Both ops are risk-neutral.** `posterior` and `voi` value decisions with
`objective: expected` against a zero-sum opponent. A scenario whose
objective or `opponent_objective` differs is warned
(`uncertainty-risk-neutral`).

## Examples

The counts and prices in these files are illustrative inputs, not facts
about any real judge or vendor.

- `examples/ptab-robust-institution.json`: robust solve on the calibrated
  PTAB pack. The prior strength at the institution decision is the
  calibration's `n`; elsewhere it is the warned default.
- `examples/cofc-posterior-observed-judge.json`: a hypothetical "this judge
  granted the government summary judgment 3 of 4 times" observation, then
  the posterior interval on the case value.
- `examples/cofc-voi-mock-sj-panel.json`: which CoFC chance nodes are worth
  learning about, and whether a $25,000 study worth five comparable
  summary-judgment rulings pays for itself.
