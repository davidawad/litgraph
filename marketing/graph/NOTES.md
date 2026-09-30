# graph: "The Docket Graph" (why litgraph is interesting)

- Piece: `marketing/graph/index.html`. Self-contained inline SVG, CSS and JS; the only
  external request is Google Fonts (IBM Plex Mono, IBM Plex Sans Condensed, Courier
  Prime, with fallback stacks). 42 s seamless loop, with pause/play and replay.
  `#t=<seconds>` opens paused on a single frame (for thumbnails).
- Reduced motion: opens paused on the solved-graph frame (t = 28.2 s). Play still works.
- No MP4. There's no system ffmpeg or H.264 encoder on the build box, and Playwright's
  bundled ffmpeg only supports its own webm recording.

## Palette / type

Ink `#0D1319`, paper `#E4E8EB`, slate `#6E7E8E`. The three roles are you `#3D9BFF`
(square), the United States `#FF5A4E` (diamond) and the court/chance in paper (circle).
Highlighter `#FFD02E` is used only for the solved line and the thesis on the end card.
Courier Prime is for the filings, Plex Mono for ids, numbers and commands, and Plex Sans
Condensed for headlines.

## Storyboard (42 s)

1. 0–5.6 "A lawsuit looks like paper." Ten rule cards sit in a pile. Each one carries its
   node's real authority and label from the `cofc` pack. The 12(b) card has the real
   `deadlines` result.
2. 5.6–12 "It is a graph." **The bold moment:** each card shrinks, rotates and rounds into
   its node's shape and lands in place. Edges draw left to right, then the wrap to row 2.
3. 12–19 "Three players move it." Nodes take their role colors and the authored
   probabilities appear on the court's edges.
4. 19–24.6 "The solver backs values up from the endings." Terminal payoffs appear first,
   then node values ripple back from `prevailing-party-fork` to `limitations-check`. The
   `explain` annotations show the US's 12(b)-vs-answer choice and the SJ regret.
5. 24.6–28.6 "Then it lights the optimal line." The highlighter runs along `best_line`
   and the other edges dim.
6. 28.6–36.6 "What it is worth, and the tail." `chain` absorption bars and expected
   totals, then the `simulate` quantile strip with P(loss), mean and CVaR, then the
   response's real `warnings`.
7. 36.6–42: end card with name, thesis, `brew install davidawad/tap/litgraph`, the GitHub
   URL, pack counts and "Not legal advice." It crossfades back into the card pile.

## Real litgraph output used (v0.2.1, `cargo build --release -p litgraph-cli`)

Raw JSON is in `/work/artifacts/litgraph-output/` (chain, solve, simulate, graph).

- `litgraph packs`: 8 packs, 470 nodes, 703 edges (summed). `cofc` has 68 nodes and 84
  edges.
- `litgraph graph --scenario cofc-1498-patent-case`: node ids, controls (me / opponent /
  chance), edge probabilities 0.95/0.05, 0.55/0.20/0.15/0.10, 0.30/0.70,
  0.50/0.15/0.20/0.15 and 0.50/0.50, plus authorities and labels.
- `litgraph deadlines --scenario cofc-1498-patent-case --arg trigger=2026-10-01 --arg node=cofc::government-response-fork`:
  60 calendar days under RCFC 6 gives 2026-11-30.
- `litgraph solve --scenario cofc-1498-patent-case`: value 48,423.07, converged in 40
  iterations, and `best_line` (the highlighted path).
- Node values: `litgraph chain --scenario cofc-1498-patent-case --arg from=cofc::<node>`,
  reading `expected_net`, e.g. 50,972 / 53,654 / 61,859 / 100,859 / 163,250 / 394,000 /
  487,000 / 500,000.
- `litgraph explain ... --arg node=cofc::government-response-fork`: answer q = 147,000,
  12(b) q = 61,859.37 (adversarial opponent).
- `litgraph explain ... --arg node=cofc::patent-dispositive-motion-fork`: trial 487,000,
  SJ 455,500, regret 31,500.
- `litgraph chain --scenario cofc-1498-patent-case`: absorption 21.7164% / 21.7164% /
  18.6141% / 16.9219% / 11.2813% / 5% / 4.75%; expected net 48,423, utility 300,927,
  dollars 252,504, elapsed 302.2 days.
- `litgraph simulate --scenario cofc-1498-patent-case`: 10,000 runs, seed 7, mean 45,432,
  p05 −407,955, p25 −92,705, p50 −47,205, p75 172,045, p95 592,045, min −635,455,
  CVaR (α 0.10) −431,706, P(loss) 0.5046.
- Warnings shown on screen as reported: `probability-fill` ×27, `mixed-node` ×5,
  `fact-unset` ×2. The $1M stake and $650/h rate are the named scenario's illustrative
  inputs, and the screen says so.

Honesty notes:
- The drawn graph is the patent-track subset. Folded single-step runs are marked `⋯n`
  with the real count.
- The two SJ-ruling-to-trial edges (0.50 denied, 0.15 in part) are drawn as one edge
  labelled "0.50 + 0.15".
- The highlighted line is the solver's `best_line`: optimal choices, with the most
  likely draw at chance nodes. It stops at `prevailing-party-fork` (0.50/0.50) rather
  than picking a winner.

## With more time

- An MP4 capture at 1920×1080 on a machine with ffmpeg/H.264.
- A phone-specific layout. At 400 px the stage scales down and the caption is mirrored
  below it, but the graph labels are tiny.
- A slower, calmer reduced-motion variant that crossfades between the five composed
  frames.
- A gentle camera push-in during the morph, and softer id-label fades on node arrival.
- Showing the solver's value iteration converging (the 40 iterations) instead of a single
  backward sweep.
