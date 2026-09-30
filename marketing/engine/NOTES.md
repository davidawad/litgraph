# engine — "Filed in Milliseconds"

What it's like to use litgraph, shown as a docket sheet: every step is a
docket entry, the JSON is set on pleading paper (numbered lines, double
red margin rule), the parts that matter get a highlighter, and each result
gets a FILED stamp showing that request's real `elapsed_ms`.

## Files

- `marketing/engine/index.html`: the piece. Self-contained. The only
  external request is Google Fonts (Archivo Narrow + IBM Plex Mono), with
  fallback stacks. There's no `<head>`/`<meta>`, so all non-ASCII is
  escaped (HTML entities, `\u` in JS) and the page renders the same under
  any charset.
- `marketing/engine/litgraph.mp4`: one 45 s loop, 1920×1080, 30 fps, H.264.
  Frames were rendered deterministically with `__render(t)` in headless
  Chromium and piped into ffmpeg.

## Storyboard (45 s loop; starts and ends on the same card, so it loops seamlessly)

1. **0–3 s: index / end card.** Wordmark, thesis, `brew install davidawad/tap/litgraph`, GitHub URL, GPL-3.0, docket index of the entries, `"ok": true` stamp. This is also the no-JS and reduced-motion frame.
2. **3–8.5 s: 01 Install.** `brew install` typed, `litgraph --version` → `litgraph 0.2.1`, `litgraph packs` rows stream in, FILED 27.945 ms.
3. **8.5–13.5 s: 02 Request.** The `examples/cofc-explain-dispositive-fork.json` request is typed as a heredoc. packs / scenario / op are highlighted in sync with the caption.
4. **13.5–22 s: 03 Response.** The real envelope streams in (abridged with `…`), FILED 13.293 ms. The camera moves through policy (chosen/regret), warnings (codes + counts), and provenance (fingerprints, modes).
5. **22–37 s: 04 Capabilities.** Six 2.5 s cards, each with a real command, an animated visual, and a real fragment: compose forums, deadline math, tornado, state flags, named scenario, calibration.
6. **37–42 s: 05 MCP.** `claude mcp add litgraph -- litgraph-mcp`, the 20 tool names from `tools/list`, then `tools/call deadlines` returns the same envelope (`due_date` 2026-11-27).
7. **42–45 s:** back to the end card.

Controls: Pause/Play, Replay, and a scrubbable rail; Space toggles.
`?t=12.5` freezes on a moment (used for screenshots). Under
`prefers-reduced-motion` it shows the composed end card and waits for
Play. Below 720 px wide it switches to a 3:4 portrait layout with the
same scenes stacked; there's no horizontal scroll at 400 px.

## Real litgraph output used

Built with `cargo build --release -p litgraph-cli -p litgraph-mcp` (v0.2.1).
The raw JSON is in `/work/artifacts/out/` of the run that made this; any
of it can be regenerated with the commands below.

- `litgraph --version` → `litgraph 0.2.1`.
- `litgraph packs`: 8 packs. Per-pack nodes/edges are shown as reported. The totals 470 nodes / 703 edges are my sums. 18 links counted from `packs/links.json`. 19 ops and 4 named scenarios are from `describe`.
- **Hero:** `litgraph q - < examples/cofc-explain-dispositive-fork.json`
  - value 2561247.961725
  - `e-dispositivefork-trial` chosen, regret 0.0; `e-dispositivefork-sj` regret 15926.859596
  - warnings compile×61, fact-unset×2, mixed-node×4, probability-fill×267
  - fingerprints `fnv1a64:67e772863ca46c68` / `87c86ec10b436caa`, 734 nodes / 828 edges, elapsed 13.293 ms
- **Compose:** `{"packs":["ptab-patent-trial-appeal-board","cafc"],"op":{"op":"graph","node":"federal-circuit-window"}}`
  - edge `links::ptab-appeal-to-cafc` → `cafc@ptab::entry-ptab`, authority 35 U.S.C. §§ 141(c), 319; 37 C.F.R. § 90.3
  - 978 nodes / 1,167 edges compiled (flag states included), 14.871 ms
  - The dot grids are exactly 55 and 58 dots, matching the pack node counts.
- **Deadlines:** `{"packs":["frcp-civil-procedure"],"op":{"op":"deadlines","trigger":"2026-11-05","node":"service-completed"}}`
  - FRCP 12(a)(1)(A)(i), 21 days → base 2026-11-26 (Thanksgiving) → due 2026-11-27, `last_day_rolled: true`, step text verbatim, 2.497 ms
- **Tornado:** `litgraph q - < examples/cofc-1498-tornado.json`
  - base 2747220.71; top five rows' v_low/v_high are drawn to scale
  - `p:cafc@cofc::panel-to-reversed` swing 1050700.38 with `policy_changes: true`, 239.434 ms
- **State flags:** `{"packs":["ptab-patent-trial-appeal-board"],"links":false,"op":{"op":"graph","node":"post-fwd-options{ipr-estopped}"}}`
  - the three `fwd-issued` edges `sets: ["ipr-estopped"]` (35 U.S.C. § 318(a), p .55/.25/.20 from the pack)
  - compile warning "104 additional node(s)", 3.179 ms
  - The on-screen claim is only that the flag is carried (CRITIQUE.md). No shipped edge `forbids` it yet, so the piece doesn't claim a blocked move.
- **Named scenario:** `litgraph chain --scenario cofc-1498-patent-case`
  - all seven absorption probabilities
  - expected dollars 252504.270313 / elapsed 302.224688 / hours 387.905781, fact-unset×2, 0.583 ms
- **Calibration:** `examples/ptab-calibrated-institution.json`
  - institution-granted goes from 0.65 (pack) to 0.681 (`ptab-fy2024`, n=1087, 740/1,087 from the USPTO FY24 roundup)
  - the two denial edges fall to 0.1595 each (residual split, warned), 3.738 ms
  - The 0.65 comes from the same request without `calibration`.
- **MCP:** `litgraph-mcp` over stdio
  - `initialize`, `tools/list` (20 tools), `resources/list` (15)
  - `tools/call deadlines` with the arguments above, same envelope as the CLI

`elapsed_ms` values come from one run on the build box and will vary run to run.

Values are shown in the engine's own units. The piece never calls them
dollars of real damages: the payoffs are placeholders, as the README says.
The footer carries "Not legal advice."

## Deliberately avoided

The `ptab-ipr-defense` named-scenario chain. Under the default scenario
it absorbs 100% into `claims-disclaimed-preinstitution`, with
not-converged warnings. That's accurate output, but it would read as a
bug in a montage, so the CoFC § 1498 scenario is used instead.

## With more time

- Portrait layout: code is ~8 px at a 400 px phone width. It's readable
  as a thumbnail; a real phone cut would trim lines to ~40 chars and
  show fewer of them.
- Add a paper-grain / ink-bleed texture to the stamps (currently flat,
  with multiply blending).
- Show an `ok:false` envelope with its `hint` (e.g. the "not found … did
  you mean" error), a strong agent-native beat left out for time.
- The brief mentions "court days": the deadline card shows FRCP 6
  calendar-day counting with a holiday roll. An RCFC/FRAP or ITC
  business-day example would show the other rule sets.
- Headless-render caveat: in this sandbox, Chromium only loaded Google
  Fonts over http (not `file://`). Browsers with system fonts are
  unaffected.
