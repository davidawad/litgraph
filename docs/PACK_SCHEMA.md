# Pack schema (v2)

A **pack** is one forum's procedure as a directed graph: nodes are procedural
postures, edges are transitions (moves, rulings, draws). Packs are JSON files in
`packs/`. v1 packs (the civ-pro-the-gathering statechart format) load unchanged;
v2 adds the fields marked **v2** below. Every v2 field is optional so a v1 file
is a valid v2 file with `schemaVersion: 1`.

The engine never guesses silently. Anything it has to infer (a payoff from a
label, a uniform draw where probabilities are missing, an elapsed time from a
deadline) is reported back in every query response under `warnings` /
`assumptions`, so an agent always knows which numbers are authored and which
are fallbacks.

## Top level

| field | type | notes |
|---|---|---|
| `schemaVersion` | `1` or `2` | |
| `id` | string | pack id, also the namespace prefix when composing (`cofc::complaint-filed`) |
| `title`, `description`, `jurisdiction` | string | |
| `forum` | string | **v2** short forum key: `frcp`, `frap`, `cafc`, `cofc`, `ptab`, `itc`, `uspto`, ... |
| `startNodeId` | node id | where walks and default analyses begin |
| `groups` | `[{id,label}]` | visual/hierarchical clustering |
| `roles` | object | **v2** actor → role mapping, see below |
| `sources` | `[{id,title,url?,path?,sha256?,asOf?}]` | **v2** primary sources the pack was authored from. `cite`/`authority` strings should be traceable to one of these |
| `nodes`, `edges` | arrays | |

## Nodes

| field | type | notes |
|---|---|---|
| `id` | string | unique within pack, kebab-case |
| `kind` | `state` \| `decision` \| `terminal` | `terminal` = the matter ends here (for this pack) |
| `label` | string | |
| `cite`, `note`, `group`, `valence` (`good`/`caution`/`bad`/`neutral`), `courtListenerUrl` | | as v1 |
| `payoff` | number (USD) | **v2**, terminals only. Value of ending here **from the protagonist's (role `self`) perspective**, before costs already spent. Authored payoffs beat the engine's label heuristic, which only understands patent-prosecution vocabulary |
| `outcome` | string[] | **v2** machine tags for terminals, e.g. `["win","judgment","fee-eligible"]`, `["loss","procedural-default"]`, `["settlement"]`, `["remand"]`. Custom cost/utility functions can branch on these (`tag("fee-eligible")`) |
| `attrs` | `{string: number}` | **v2** free numeric attributes exposed to custom functions as `node.<name>` |

## Edges

| field | type | notes |
|---|---|---|
| `id` | string | **v2** stable edge id, unique within pack. If absent the loader derives `from->to#n` (n = 0-based index among parallel edges). Author it whenever two edges share `from`/`to` |
| `from`, `to`, `label` | string | |
| `actor` | `applicant` \| `examiner` \| `office` \| `either` | v1 vocabulary kept for compatibility. Interpreted through `roles` |
| `authority` | string | the rule/statute that authorizes or compels this transition |
| `deadline` | `{length, unit?, extendable?, extensionAuthority?, note?}` | `length` in days. **v2** `unit`: `calendar` (default) or `court` (court days) |
| `duration` | `{min?, mode, max?}` days | **v2** expected elapsed calendar time for this transition. Distinct from `deadline` (a window you must act inside). If absent the engine falls back to `deadline.length` and says so |
| `cost` | USD | out-of-pocket fees (filing fees, USPTO fees, bonds) |
| `hours` | number | attorney hours to execute (self/opponent moves only) |
| `probability` | 0..1 | only on non-`self` edges. Out-edges of a pure chance node must sum to 1 |
| `valence`, `courtListenerUrl`, `actionId`, `note` | | as v1 |
| `tags` | string[] | **v2** e.g. `["dispositive","sanctions","waiver-trap","settlement","appeal"]` |
| `attrs` | `{string: number}` | **v2** free numeric attributes for custom functions (`edge.<name>`), e.g. `{"opp_hours": 40, "fee_award_prob": 0.3}` |

## Roles (v2)

The game-theoretic reading of `actor`. Default mapping (the v1 semantics):

```json
"roles": { "applicant": "self", "examiner": "nature", "office": "nature", "either": "nature" }
```

Roles: `self` (the protagonist whose policy is optimized), `opponent` (an
adversary with its own choices — solved as a minimizer or with its own
objective, per query), `nature` (tribunal/agency/chance — draws per
`probability`). A pack whose `examiner` column is really "opposing party"
should say `"examiner": "opponent"`. Queries can override the mapping
(`perspective`), e.g. analyze the same FRCP pack as the defendant.

In packs where "applicant" is not a patent applicant (FRCP, CoFC, FRAP), the
convention is: `applicant` = the party whose moves we are analyzing (usually
plaintiff/appellant), `examiner` = the opposing party, `office` = the court or
clerk, `either` = either party / events not controlled by one side.

## Links (composition)

`packs/links.json` joins packs into one multi-forum graph:

```json
{ "links": [
  { "id": "cofc-to-cafc", "from": "cofc::judgment-entered", "to": "cafc::notice-of-appeal-filed",
    "label": "Appeal to the Federal Circuit", "actor": "applicant",
    "authority": "28 U.S.C. § 1295(a)(3); 28 U.S.C. § 2522",
    "deadline": { "length": 60 } }
]}
```

When composing, a terminal with outgoing link edges becomes a choice node with
an implicit zero-cost `accept` edge to a copy of itself holding its payoff
(for `self`-controlled links), so "stop here or appeal" is a real decision.

## Authoring rules

1. Every `cite` / `authority` must be checkable against a `sources` entry or a
   public citation. No invented rule numbers.
2. Probabilities are optional. Where you author one, put the basis in `note`
   (statistic + vintage, or "teaching estimate"). Unauthored is better than
   invented; the engine reports unauthored chance nodes.
3. Hours are BigLaw-honest estimates for the move; `cost` is cash only.
4. Terminals should carry `payoff` and `outcome` in v2 packs.
