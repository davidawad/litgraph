# Settlement prediction (`settle`)

`settle` answers "what should this case settle for, and when?" from the
game the engine already solves. It adds no new modeling input beyond the
opponent's own objective: every number comes from the solved general-sum
equilibrium (`scenario.opponent_objective`, see `docs/CRITIQUE.md`'s
"General-sum opponents").

```bash
litgraph q - < examples/cofc-1498-settle.json
litgraph settle --scenario '{"extends":"cofc-1498-patent-case","opponent_objective":"-payoff"}' --arg node=cofc::answer-filed
```

## Inputs

| field | meaning | default |
|---|---|---|
| `scenario.opponent_objective` | the opponent's own terminal payoff (e.g. `"-payoff"` for a defendant who pays the judgment); its per-edge cost is the `opponent_dollars` metric | unset → zero-sum, `settle-zero-sum` warning, never a deal |
| `scenario.objective` | `self`'s risk attitude: `expected`, `cara`, `worst`, or `cvar` | `expected` |
| `node` | evaluate from this node | the start |
| `self_side` | `plaintiff` or `defendant`; prices are always what the defendant pays | `plaintiff` |
| `opponent_risk` | the opponent's risk attitude, same shape as `scenario.objective` | `{type: expected}` |
| `bargaining_power` | the plaintiff's Nash weight `β` in `[0, 1]` | `0.5` |
| `discount_annual` | `{plaintiff?, defendant?}` annual rates driving bargaining patience | `scenario.discount_annual`, else none |
| `delta` | `{plaintiff?, defendant?}` per-round discount factors in `(0, 1]`, overriding the rates | — |
| `rule68` | `{offer, costs?, eligible?}` an offer of judgment served at `node` | — |
| `runs`, `seed` | Monte Carlo budget for a `cvar` side | `20000`, `7` |
| `max_steps` | timing-line length cap | `40` |

## Walk-away points and the ZOPA

The policy is the subgame-perfect equilibrium `solve` reports. Under that
fixed policy each side's **continuation value** at a node is its certainty
equivalent of litigating on, under its own terminal payoffs, its own
costs, and its own risk attitude:

- `expected`: the expectation (identical to `solve`'s `value` /
  `opponent_value`);
- `cara` (absolute risk aversion `a`): `−(1/a)·ln E[e^(−a·X)]`, exact by
  backward recursion;
- `worst`: every draw goes against that side;
- `cvar` (`alpha`): the mean of the worst `alpha` fraction of that side's
  outcome, **estimated** by seeded Monte Carlo under the policy
  (`settle-cvar-sampled` warning). CVaR is not a backward recursion, so the
  stopping policy and off-line prices use that side's expectation.

The continuation value is the side's walk-away point. The plaintiff accepts
any price `P ≥ r_p` (its certainty equivalent); the defendant pays any
`P ≤ r_d` (minus its certainty equivalent). When `r_p < r_d` the result
carries `zopa: {low: r_p, high: r_d, surplus}`; otherwise `deal: false` and
`no_deal_gap = r_p − r_d ≥ 0`. A **zero surplus is no deal**: there is
nothing to gain by settling. With risk-neutral sides the surplus is exactly
the two sides' expected remaining litigation costs — the classic
Landes/Posner/Gould result that settlement happens because trial burns
money both sides would rather split (Gould 1973).

## Prices, side by side

| model | price | notes |
|---|---|---|
| split the surplus | `r_p + S/2` | the midpoint (Gould 1973) |
| Nash bargaining, symmetric | maximizes `(u_p(P) − u_p(r_p))·(u_d(−P) − u_d(−r_d))` | Nash (1950) |
| Nash bargaining, weighted | maximizes `(u_p(P) − u_p(r_p))^β·(u_d(−P) − u_d(−r_d))^(1−β)` | Kalai (1977); `β = bargaining_power` |
| Rubinstein, plaintiff first | `r_p + S·(1 − δ_d)/(1 − δ_p·δ_d)` | Rubinstein (1982) |
| Rubinstein, defendant first | `r_p + S·δ_p·(1 − δ_d)/(1 − δ_p·δ_d)` | Rubinstein (1982) |
| Rubinstein limit | `r_p + S·ρ_d/(ρ_p + ρ_d)`, `ρ = −ln δ` | round length → 0: the Nash split with power `ρ_d/(ρ_p + ρ_d)` (Binmore, Rubinstein & Wolinsky 1986) |

`u` is each side's utility for a *sure* payment: linear, or CARA
(`(1 − e^(−a·x))/a`) for a `cara` side. So with two risk-neutral sides the
symmetric Nash price equals the midpoint and the weighted one is `r_p + β·S`;
a more risk-averse side concedes part of the surplus. Rubinstein divides the
surplus in money (transferable utility).

**Rubinstein's discount factors** are `δ_i = (1 + r_i)^(−Δ/365)`: `r_i` the
side's annual rate (`discount_annual`, else `scenario.discount_annual`) and
`Δ` the round length — the expected calendar days of the next procedural
step that takes time under the policy (`round_days`; zero-duration forks are
looked through). `delta` overrides a side's factor directly. With no rates
and no durations both factors are 1 and the split is even. The *costs* of
the next step enter through the walk-away points (each continuation value
nets that side's future spend), not through `δ`.

## When: timing and optimal stopping

Along the most likely line from `node` (the policy's choices and the likeliest
draw at each chance point), `timing.line` recomputes the range at every
node; `timing.peak` is where the surplus peaks — typically just after a
ruling that makes the long, expensive path likelier (a denied motion to
dismiss) or just before a large block of spend.

Settling is then treated as an **always-available action**: at every node
where a deal zone exists either side may settle at the weighted Nash price
there. For each side,

```text
W(n) = max( settle(n), cont(n) )
cont(n) = risk-aggregate over the step from n of  −cost(e) + γ(e)·W(e.to)
```

where the side re-optimizes its own choices knowing it can settle later and
the other side's choices and nature's draws stay as solved (a one-sided
stopping problem per side, not a joint re-solve). `self_settles` /
`opponent_settles` flag where settling now strictly beats continuing;
`first_settle` is `self`'s first such node on the line;
`value_with_settlement − value_litigate` is the value of the option; and
`policy_changes` lists the line nodes where `self`'s move changes.

## Rule 68 offer of judgment

FRCP 68, verified on LII
(<https://www.law.cornell.edu/rules/frcp/rule_68>, fetched 2026-09-30,
sha256 `559d497022ff66b7791df72d20d808d93283af5528cf3719cf6a13a8cfe92b8a`):

> (a) … At least 14 days before the date set for trial, a party defending
> against a claim may serve on an opposing party an offer to allow judgment
> on specified terms, with the costs then accrued. …
> (d) Paying Costs After an Unaccepted Offer. If the judgment that the
> offeree finally obtains is not more favorable than the unaccepted offer,
> the offeree must pay the costs incurred after the offer was made.

RCFC 68 in the Court of Federal Claims is word-for-word the same (vendored
`sources/rcfc.txt`, "## RCFC 68"). Two Supreme Court decisions shape the
model, both read on LII:

- *Delta Air Lines, Inc. v. August*, 450 U.S. 346 (1981)
  (<https://www.law.cornell.edu/supremecourt/text/450/346>, sha256
  `1c72c9bb0e0beccb65ed1c98a0bc65e6a21e92c0831de4a290ee739e654712d3`): the
  rule "applies only to offers made by the defendant and only to judgments
  obtained by the plaintiff"; it is "simply inapplicable" when judgment is
  for the defendant.
- *Marek v. Chesny*, 473 U.S. 1 (1985)
  (<https://www.law.cornell.edu/supremecourt/text/473/1>, sha256
  `d0ca735d46df93816f3b87b14a7eb41c054336a9fb46e21850e4105889ebb98f`):
  "costs" means "all costs properly awardable under the relevant substantive
  statute" — attorney's fees only where that statute defines costs to
  include them.

**Model.** `rule68: {offer, costs, eligible?}` is an offer served by the
defendant at `node`. At every terminal where 68(d) bites — by default one
tagged `judgment` whose value to the plaintiff is positive (a judgment the
plaintiff obtained) and at most `offer` (not more favorable) — the plaintiff
pays the defendant `costs`. `eligible` replaces that test with a terminal
expression (`offer` is visible as a parameter). Both walk-away values are
re-evaluated under the same policy; the result reports
`plaintiff_reject_value`, `defendant_reject_value`, the shifted range,
`p_triggered` (probability the case ends in a triggering judgment), and
`plaintiff_accepts` (`offer ≥ plaintiff_reject_value`).

`costs` is **your estimate** of the defendant's post-offer costs under the
governing statute; the engine does not compute it. Not modeled: the
14-day timing requirement (serve the offer at a node before trial), the
plaintiff's loss of its own post-offer cost recovery, comparing the
judgment *plus pre-offer costs* against the offer, and re-optimizing the
litigation policy once the offer is outstanding. Miller (1986) analyzes the
rule's settlement incentives in the same bargaining-range framework.

## Worked example: `cofc-1498-patent-case`

`examples/cofc-1498-settle.json` extends the named §1498 scenario with
`opponent_objective: "-payoff"` (the United States pays the judgment and
bears its own `opp_rate` spend). The patent-owner and government annual
rates (10% / 4%) and the Rule 68 figures ($450,000 offer, $25,000 costs) are
**illustrative estimates**, not sourced numbers; the payoffs and rates come
from the scenario, which labels its own as illustrative. At claim accrual:

- walk-away points $54,473 (patentee) and $368,869 (government): a ZOPA with
  $314,396 of surplus — the two sides' expected remaining spend;
- midpoint and symmetric Nash $211,671 (both sides risk-neutral);
  Rubinstein ≈ $146,000 either order (54-day first round, the more patient
  government keeps ≈71% of the surplus);
- the surplus peaks at `cofc::answer-filed` ($361,500), right after the
  government's RCFC 12(b) motion is denied — the dismissal risk is gone and
  all of claim construction, expert discovery and trial are still ahead;
- with settlement always available the patentee's value rises from
  $54,473 to $211,671, and it settles at the filing fork rather than filing;
- the scenario's judgments are all-or-nothing ($1,000,000 or $0), so a Rule
  68 offer below $1,000,000 never triggers 68(d) (`p_triggered: 0`): the
  offer matters here only as a sure alternative the patentee should accept
  when it beats its walk-away value.

The response also carries the scenario's own warnings (`fact-unset`,
`mixed-node`, `probability-fill`): report them with the numbers.

## Limits

- The litigation policy is the solved equilibrium without a settlement
  option; the stopping pass lets each side re-optimize its own moves given
  the option but keeps the other side's moves fixed.
- A `cvar` side is priced by sampling (see above); a `cvar`
  `scenario.objective` doesn't change the policy (`settle-cvar-policy`),
  exactly as for `solve` with an `opponent_objective`.
- Complete information: both sides see the same probabilities. Divergent
  beliefs ("mutual optimism") can be expressed through `opponent_objective`
  (e.g. `"-0.5 * payoff"`), which is how a negative gap arises.
- Everything the underlying solve assumes (probability fills, act-or-wait
  nodes, unauthored payoffs) carries through; its warnings are attached.

## References

- Nash, J. F. (1950). The Bargaining Problem. *Econometrica* 18(2), 155–162. doi:10.2307/1907266
- Kalai, E. (1977). Nonsymmetric Nash solutions and replications of
  2-person bargaining. *International Journal of Game Theory* 6(3),
  129–133. doi:10.1007/BF01774658
- Rubinstein, A. (1982). Perfect Equilibrium in a Bargaining Model.
  *Econometrica* 50(1), 97–109. doi:10.2307/1912531
- Binmore, K., Rubinstein, A., & Wolinsky, A. (1986). The Nash Bargaining
  Solution in Economic Modelling. *RAND Journal of Economics* 17(2),
  176–188. doi:10.2307/2555382
- Gould, J. P. (1973). The Economics of Legal Conflicts. *Journal of Legal
  Studies* 2(2), 279–300. doi:10.1086/467499
- Miller, G. P. (1986). An Economic Analysis of Rule 68. *Journal of Legal
  Studies* 15(1), 93–125. doi:10.1086/467805

Bibliographic details checked against Crossref (`api.crossref.org/works/<doi>`)
on 2026-09-30.
