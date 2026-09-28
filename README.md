# litgraph

Litigation procedure as a stochastic game. Forum procedures (FRCP, FRAP,
Federal Circuit, Court of Federal Claims, PTAB, ITC § 337, USPTO
prosecution, FRCrimP) are JSON graphs; litgraph composes them into one
graph and answers: what to do at each step, expected cost/time, the
distribution of endings, what the answer is most sensitive to, and what
changes under any counterfactual — with custom cost/utility/probability
functions and every assumption reported back.

Extracted from `civ-pro-the-gathering` (the game keeps its UI; the engine
lives here), rewritten in Rust, and verified against the original
TypeScript engine on all six original packs.

```bash
cargo build --release
./target/release/litgraph describe
./target/release/litgraph packs
./target/release/litgraph explain --packs cofc --arg node=complaint-filed
./target/release/litgraph q '{
  "packs": ["cofc", "cafc"],
  "scenario": {
    "params": { "rate": 850 },
    "payoffs": { "cofc::judgment-for-plaintiff": 4000000 },
    "objective": { "type": "cara", "a": 2e-7 },
    "fee_shift": { "fraction": 0.4 }
  },
  "op": { "op": "simulate", "runs": 50000 }
}'
```

Runnable requests live in `examples/` (a 28 U.S.C. § 1498 patent case in
the Court of Federal Claims through the Federal Circuit: expected outcome,
risk-averse simulation, the § 1500 trap as a counterfactual, sensitivity,
the stake at which an appeal is worth briefing):

```bash
./target/release/litgraph q - < examples/cofc-1498-chain.json
```

Docs: [AGENTS.md](AGENTS.md) (operating manual) ·
[ARCHITECTURE](docs/ARCHITECTURE.md) ·
[PACK_SCHEMA](docs/PACK_SCHEMA.md) ·
[COST_FUNCTIONS](docs/COST_FUNCTIONS.md) ·
[CRITIQUE](docs/CRITIQUE.md) (what was wrong with v1 and what changed).
