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
| `sources` | `[{id,title,url?,path?,sha256?,asOf?}]` | **v2** primary sources the pack was authored from. `cite`/`authority` strings should be traceable to one of these. `path` is **repo-relative only** (e.g. a sibling pack file) — never a local/absolute filesystem path; cite the official `url` for everything else. `litgraph lint` errors (`local-source-path`) on an absolute `path` |
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
| `tags` | string[] | **v2** free-form node tags for any node (not just terminals), e.g. `["entry","router"]`. Expressions see a node's `outcome` ∪ `tags` as one set: `tag("x")` on a terminal, `to_tag("x")`/`from_tag("x")` on an edge testing its target/source node |
| `attrs` | `{string: number}` | **v2** free numeric attributes exposed to custom functions as `node.<name>` |
| `payoffByFlag` | `{string: number}` | **v2**, terminals only. Overrides `payoff` on a flagged product-graph copy of this terminal: if the copy carries one of these keys as a set flag, its payoff is that value instead of `payoff`. See [State flags](#state-flags) |

## Edges

| field | type | notes |
|---|---|---|
| `id` | string | **v2** stable edge id, unique within pack. If absent the loader derives `from->to#n` (n = 0-based index among parallel edges). Author it whenever two edges share `from`/`to` |
| `from`, `to`, `label` | string | |
| `actor` | `applicant` \| `examiner` \| `office` \| `either` | v1 vocabulary kept for compatibility. Interpreted through `roles` |
| `authority` | string | the rule/statute that authorizes or compels this transition |
| `deadline` | `{length, unit?, extendable?, extensionAuthority?, note?}` | `length` in days. **v2** `unit`: `calendar` (default) or `court` (court days). The `deadlines` API op turns this into a concrete due date from a trigger date, choosing `FRCP 6`/`RCFC 6`/`FRAP 26`/`19 CFR 210.6(a)` by the pack's `forum` (see `docs/ARCHITECTURE.md`'s "Deadline clock") |
| `duration` | `{min?, mode, max?}` days | **v2** expected elapsed calendar time for this transition. Distinct from `deadline` (a window you must act inside). If absent the engine falls back to `deadline.length` and says so |
| `cost` | USD | out-of-pocket fees (filing fees, USPTO fees, bonds) |
| `hours` | number | attorney hours to execute (self/opponent moves only) |
| `probability` | 0..1 | only on non-`self` edges. Out-edges of a pure chance node must sum to 1 |
| `valence`, `courtListenerUrl`, `actionId`, `note` | | as v1 |
| `tags` | string[] | **v2** e.g. `["dispositive","sanctions","waiver-trap","settlement","appeal"]` |
| `attrs` | `{string: number}` | **v2** free numeric attributes for custom functions (`edge.<name>`), e.g. `{"opp_hours": 40, "fee_award_prob": 0.3}` |
| `replaces` | string[] | **v2, `links.json` only** — qualified (`pack::edge-id`) local edge ids this link edge supersedes when it's active. The compiled graph drops every listed edge once the link is loaded; error if any id doesn't resolve. Invalid on a pack's own edges (packs describe one forum; superseding is a composition concern, so it belongs in `links.json`) |
| `sets`, `clears` | string[] | **v2** state flags this edge sets/clears when taken. See [State flags](#state-flags) |
| `requires`, `forbids` | string[] | **v2** flags that must all be set / all be absent for this edge to exist in the compiled graph. See [State flags](#state-flags) |

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

## Matter facts (v2)

Some chance nodes aren't real uncertainty — they're a **fact about this
matter** that just hasn't been told to the engine yet: whether a claim was
filed more than six years after accrual (`28 U.S.C. §2501`), whether the
same claim is already pending elsewhere (`28 U.S.C. §1500`), whether a
judgment arises under the patent laws (`28 U.S.C. §1295(a)(1)`). Modeling
these as an ordinary unauthored chance node makes them 50/50 (or whatever
`scenario.prob_fill` does), which is never right: the true answer is
either 0 or 1, the pack's author just doesn't know which until the
scenario says so.

**Tag the node** `"tags": ["fact"]` and **author a prior** — a labeled
estimate or sourced base rate — as the `probability` on every one of its
out-edges, same as any chance node, with the basis in each edge's `note`
(e.g. "estimate: counsel screens for this before filing"). `litgraph lint`
flags a `fact`-tagged node whose out-edges aren't all authored
(`fact-no-prior`) — the whole point of tagging it is that a real prior,
not a 50/50 default, should be there.

At query time, a scenario that knows the actual fact sets
**`scenario.facts`**: `{node ref: edge ref}`, exactly the shape of
`policy` (which forces *our* choice) but for a fact node nature controls —
it forces that edge's probability to 1 and its siblings to 0 (via the same
sibling-rescale `scenario.probabilities` already does; `facts` is sugar
over it, keyed by node so an agent doesn't have to compute the edge id by
hand). A `fact`-tagged node with no `scenario.facts` entry falls back to
the authored prior and a `fact-unset` warning names it, so "I don't know
this matter's facts yet" is always visible in the response, never silent.

```json
"scenario": { "facts": { "cofc::limitations-check": "cofc::e-limitations-timely" } }
```

Use `tags: ["fact"]` (not a new node `kind`) precisely because a fact node
is structurally an ordinary chance node — same edges, same probabilities,
same `fill` semantics — only its *epistemic status* differs, and every
existing algorithm, lint check and scenario field keeps working unchanged.

## Links (composition)

`packs/links.json` joins packs into one multi-forum graph. Its top-level
shape is `{"links": [...], "instances": {...}}`.

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

A link edge only loads once both endpoints' packs are loaded; it's silently
skipped otherwise, so a two-pack request never sees a link into a third,
unloaded pack. A link can carry `replaces` (see the edge table above) to
supersede a pack's own edge — for instance, a link representing "appeal to
the Federal Circuit" superseding a same-pack placeholder terminal edge.

## State flags

**v2.** Litigation has memory: an IPR estoppel, a waived Rule 12(h) defense, a prior
RCE all change what can happen next without changing where you structurally
are. A plain graph can't express "this edge only exists if X happened
earlier" — `docs/CRITIQUE.md` calls this the *Markov on the node* limit.
State flags are the fix: `sets`/`clears`/`requires`/`forbids` on an edge
(pack or `links.json`), compiled at load time into a **product graph** over
`(node, flag-set)`.

```json
{ "from": "fwd-issued", "to": "fwd-all-unpatentable", "label": "...",
  "sets": ["ipr-estopped"] },
{ "from": "later-invalidity-defense", "to": "...", "label": "...",
  "forbids": ["ipr-estopped"] }
```

- **`sets` / `clears`** — flags this edge adds to / removes from the current
  flag-set when taken. Flags are plain strings, not declared anywhere up
  front; a typo just means the flag is never set (no error, but `litgraph
  lint` can flag a `requires`/`forbids` that references one no edge ever
  sets).
- **`requires` / `forbids`** — gate the edge: it only exists in the compiled
  graph from a state where every `requires` flag is set and every `forbids`
  flag is absent. An edge whose `forbids` condition is always true wherever
  it's reachable (e.g. immediately after the flag that forbids it) simply
  never compiles anywhere — the block is absolute, not a soft preference.

**Compilation.** Every base node keeps its own empty-flag copy at its
original id and index — so a pack that never uses flags compiles to an
*identical* graph (`Graph::compile` skips the whole mechanism when no loaded
edge declares any of the four fields; a pack with a `sets`/`clears`/
`requires`/`forbids` field on `RawEdge` is still ordinary v1/v2 JSON,
these are additive, defaulted fields), and any node stays directly
addressable as `scenario.start` regardless of true reachability, same as
before flags existed. From there, a worklist walks `(node, flag-set)` states
reachable by forward flag propagation; the first time a state is reached its
node is materialized as `pack::local{flag1,flag2}` (flags sorted,
comma-joined — e.g. `ptab::fwd-issued{ipr-estopped}`), stable and readable.
**Only reachable combinations are materialized** — a pack with one flag used
in one corner of the graph gets one small pocket of duplicated nodes, not a
combinatorial blowup. A hard cap (`max_product_nodes`, default 20,000;
`CompileOptions.max_product_nodes` / request `max_product_nodes`) fails
compilation with a clear error naming the state it choked on, rather than
compiling forever or truncating silently.

**Everything downstream is unchanged.** Every algorithm (`solve`, `chain`,
`simulate`, `path`, `pareto`, `sweep`, `structure`) sees the product graph as
an ordinary graph — flags are compiled away into plain nodes before any
algorithm runs, so none of them know flags exist. A compiled `Node` carries
`flags` (its flag-set) and `base_id` (the unflagged id it's a copy of), so a
response can always project a flagged node back to the pack node an author
wrote (`api::render::node_ref` does this whenever `flags` is non-empty). A
compiled `Edge` carries `base_id` too (the pre-expansion edge it's an
instantiation of) — every flagged instantiation of the same authored edge
shares one `base_id`, which is how a calibration entry (`docs/CALIBRATION.md`)
targets one edge by its authored id and lands on every flagged copy at once,
not just the one whose exact id matches.

**Reading flags in expressions.** `flag("x")` is available in edge
(cost/mask/probability-transform) and terminal (utility) expressions — see
`docs/COST_FUNCTIONS.md`. On an edge it reads the source node's flag-set; on
a terminal it reads that terminal's own flag-set.

**Terminal payoff by flag.** A terminal can carry `payoffByFlag` (Nodes
table above): when a flagged copy of that terminal is materialized, a
matching key overrides `payoff` for that copy specifically (first match in
sorted key order if more than one flag matches — author mutually exclusive
flags). This is how `cafc-federal-circuit.json`'s `cert-not-sought`/
`cert-denied` restore the panel's actual win/loss through a rehearing-or-cert
detour that would otherwise re-zero it (`panel-affirmed`/`panel-reversed`/
`panel-mixed`, set on `panel-decision`'s outgoing edges and cleared on
`rehearing-granted-to-decision` since a fresh disposition replaces the one
being reheard).

**A reachability note.** Reachability lint (`litgraph lint`'s `unreachable`
diagnostic) checks per *base id*, not per exact compiled copy: a node that's
only ever reached with a flag set (nothing reaches its empty-flag copy in
practice) is not flagged as unreachable as long as *some* compiled copy of
it is reachable from the pack's start.

**Relation to `links.json` instances.** An instance (below) is a coarser,
same-mechanism cousin: both are "the product-graph construction for one
piece of history, expressed as data instead of code" (that phrase up front
in the Instances section was written before flags existed and is still
accurate for instances). An instance is a whole namespaced *copy of a pack*
— appropriate when the history changes which pack you're even in (CAFC
entered from the PTAB vs. from the CoFC needs a different remand target and
a flipped perspective) or needs its own `payoff_transform`/`roles` override.
A flag is a *value on one edge* — appropriate for memory that lives inside
one pack's own graph (an estoppel, a waiver, a count) and needs no identity
or perspective change. The two compose freely: an instance's own edges can
carry `sets`/`clears`/`requires`/`forbids` like any other edge (materialized
once the instance is compiled), and a link edge in `links.json` can too.
Reimplementing instances purely on top of flags was considered and rejected:
an instance's `remove_edges`/`probabilities`/`roles`/`payoff_transform`/
`patch_edges` rewrite a whole pack's data before compilation even starts,
which flags (a runtime product over an already-compiled edge set) have no
mechanism for.

## Instances (`links.json`, `"instances"`)

A **pack instance** is a namespaced copy of a base pack that remembers *how
it was entered* — the product-graph construction for one piece of history,
expressed as data instead of code. The Federal Circuit is one pack (`cafc`)
but four instances: entered from the Court of Federal Claims as the
plaintiff-appellant (`cafc@cofc`), from the CoFC as the government-appellant
(`cafc@cofc-gov`, perspective flipped), from the PTAB (`cafc@ptab`), and
from the ITC (`cafc@itc`) — each remanding only back to its own origin
forum, which the shared `cafc` pack alone can't express.

```json
{ "instances": {
  "cafc@cofc-gov": {
    "pack": "cafc",
    "note": "Federal Circuit as entered by the United States appealing a CoFC plaintiff win. Perspective flipped: the government (appellant) is the opponent; payoffs are the plaintiff's (stake − appellant payoff, stake = $1M placeholder).",
    "remove_edges": ["origin-to-district-court", "origin-to-ptab", "origin-to-itc",
                      "remand-route-district-court", "remand-route-ptab", "remand-route-itc"],
    "probabilities": { "remand-route-cofc": 1.0 },
    "roles": { "applicant": "opponent", "examiner": "self" },
    "payoff_transform": "1000000 - payoff",
    "patch_edges": {
      "rehearing-file": { "deadline": { "length": 45, "note": "45 days when the United States is a party (Fed. Cir. R. 40)." } }
    }
  }
}}
```

| field | type | notes |
|---|---|---|
| `pack` | string | base pack id to copy |
| `note` | string | why this instance exists — required in practice, since the whole point is to say what's different |
| `remove_edges` | string[] | local edge ids (as the compiler derives them: authored `id`, else `from->to#n`) to drop from this instance |
| `probabilities` | `{edge id: p}` | override/author a probability on a local edge in this instance only |
| `roles` | `{actor: role}` | override the base pack's `roles` for this instance (a perspective flip) |
| `payoff_transform` | expression over `payoff` | rewrites every terminal payoff, e.g. `"1000000 - payoff"` to convert an appellant's payoff into the original plaintiff's stake-relative payoff |
| `patch_edges` | `{edge id: JSON merge-patch}` | RFC 7386 merge-patch applied to one local edge's fields (deadline, cost, hours, tags, ...) — `null` deletes a field, an object merges, anything else replaces |

Referencing an edge id `remove_edges`/`probabilities`/`patch_edges` doesn't
recognize is an error naming the unknown id — instances are checked at
compile time, same as everything else.

## Scenario library (`scenarios/*.json`)

A **named scenario** is a matter profile: a human summary/notes, the packs it
needs, its sourced facts, and the engine scenario itself (rates, stakes,
perspective, masks, payoffs, probabilities, policy, modeling modes — the same
`scenario` object a request carries; `litgraph schema scenario` gives its
full shape). It is embedded into the binary the same way packs are
(`crates/litgraph/build.rs`); `$LITGRAPH_SCENARIOS` or `--scenarios-dir`
override it at runtime, independently of `$LITGRAPH_PACKS`/`--packs-dir`.

| field | type | notes |
|---|---|---|
| `id` | string | name a request references (`"scenario": "<id>"`); by convention matches the file stem |
| `summary` | string | one line; `litgraph describe`'s `scenario_library` lists every scenario by this |
| `notes` | string | facts, the basis for every rate/stake/probability (sourced vs illustrative), and any modeling judgment calls |
| `packs` | string[] | pack refs this scenario needs. A request naming this scenario with no `packs` of its own uses these; an explicit request `packs` always wins |
| `sources` | `[{title,url?,as_of?}]` | sourced facts backing this scenario's numbers |
| `scenario` | object | the engine scenario (`litgraph schema scenario`) |

`litgraph schema named-scenario` gives the full JSON Schema; `litgraph
validate scenarios/your-scenario.json` (or `--kind named-scenario`) resolves
it against the current packs the same way a request is validated, reporting
unresolved node/edge refs and expression errors.

### Referencing a scenario from a request

A request's `scenario` field is one of:

- absent / `null` — the engine default (no overrides).
- an inline object — today's shape, unchanged; unknown fields are still a
  hard error naming the valid ones.
- a bare string `"<id>"` — the named scenario's `scenario`, unmodified; its
  `packs` apply when the request gives none of its own, so a request can be
  just `{"scenario": "<id>", "op": {...}}`.
- `{"extends": "<id>", ...overrides}` — the named scenario's `scenario`
  deep-merged (RFC 7386) with `overrides` (every other key); the merged
  result is still parsed as a real `Scenario`, so a typo in `overrides` is
  the same "unknown field" error as an inline scenario, not a silent no-op.

CLI: `--scenario <name|file.json|'{"inline":"json"}'>`; `--set k=v` composes
onto a named `--scenario` via `extends` automatically. `--scenarios-dir` (or
`$LITGRAPH_SCENARIOS`) points at a directory instead of the embedded library.

## Authoring rules

1. Every `cite` / `authority` must be checkable against a `sources` entry or a
   public citation. No invented rule numbers. A `sources[].path` must be
   repo-relative (or omitted) — never a local filesystem path; cite the
   official `url` instead. If `path` points at a vendored L0 text file
   under `sources/` (see `sources/PROVENANCE.md`), `litgraph lint`
   verifies the cite mechanically — normalizing the surface form ("FRCP
   12(b)(6)", "Fed. R. Civ. P. 12(b)(6)", "28 U.S.C. § 1498(a)", "37
   C.F.R. § 42.108", "RCFC 56", ... all resolve the same way), resolving
   it to a `## <heading>` span in that file, and fuzzy-matching any
   quoted phrase in `note` against that span. A cite it can't verify gets
   an `unverifiable-cite` warning naming the exact reason (no local
   `sources[].path`, no matching heading, quote not found, ...) rather
   than silently passing; add new vendored text under `sources/` before
   citing a body of law that isn't there yet.
2. Probabilities are optional. Where you author one, put the basis in `note`
   (statistic + vintage, or "teaching estimate"). Unauthored is better than
   invented; the engine reports unauthored chance nodes.
3. Hours are BigLaw-honest estimates for the move; `cost` is cash only.
4. Terminals should carry `payoff` and `outcome` in v2 packs.
5. Parsing is strict: an unknown field anywhere in a pack, edge, source, or
   instance is a hard error (not a silently-ignored typo). Check a file
   before opening a PR: `litgraph validate packs/your-pack.json` (or
   `--kind pack` to force detection), and `litgraph lint --packs your-pack`
   for content diagnostics once it parses.
