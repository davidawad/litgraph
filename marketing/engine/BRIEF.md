# Brief: litgraph marketing motion graphics

You are making ONE self-contained animated web page (a looping motion-graphics
piece, ~30-45 s per loop) that sells litgraph. Another agent is making a second
piece from a different angle, so stay in your lane (your angle is in your prompt).

## The product (read, don't guess)
litgraph: https://github.com/davidawad/litgraph (GPL-3.0, v0.2.1). You are in
its repository; read README.md, AGENTS.md,
docs/ARCHITECTURE.md, docs/COST_FUNCTIONS.md, docs/CRITIQUE.md, CHANGELOG.md.
It models litigation procedure (FRCP, FRAP, FRCrimP, Court of Federal Claims,
Federal Circuit, PTAB, ITC 337, USPTO prosecution) as a stochastic game on a
graph: roles self / opponent / nature; solve (optimal policy), chain (outcome
distribution), simulate (Monte Carlo with CVaR), paths/pareto, sweeps and
tornado sensitivity, deadlines (FRCP 6 / RCFC 6 / FRAP 26 court-day math),
state flags (e.g. IPR estoppel), general-sum opponents (Nash), calibration from
published USPTO/court statistics, cite verification against vendored rule text.
JSON in, JSON out. CLI + MCP server + WebAssembly. `brew install davidawad/tap/litgraph`.

## Use real output, never invent numbers
Build it first: `cargo build --release -p litgraph-cli` and use
`./target/release/litgraph` (on screen, show it as `litgraph`). Run real requests and put the real
numbers/ids/JSON on screen: e.g. `litgraph describe`, `litgraph packs`,
`litgraph chain --scenario cofc-1498-patent-case`, `litgraph solve ...`,
`litgraph simulate ...`, `litgraph deadlines ...` (check `litgraph describe`
and `litgraph schema request` for exact shapes). Real node ids, real edge
labels, real probabilities, real counts (nodes/edges/packs). If you round, round
honestly. Any legal claim on screen must be true of the packs. Include a small,
tasteful "Not legal advice." line somewhere (the README carries this disclaimer).

## Deliverable
- One file: `marketing/engine/index.html` in this repo (piece named below). Self-contained:
  inline CSS/JS; the only allowed external scripts are pinned UMD builds from
  https://cdnjs.cloudflare.com (e.g. GSAP) or https://cdn.jsdelivr.net/npm/;
  fonts only via https://fonts.googleapis.com with real fallback stacks.
  No other hosts (images/fetch to other hosts are blocked). Prefer Canvas/SVG
  you generate in JS over giant hand-written path data.
- Do NOT write <!doctype>, <html>, <head>, <body> tags; start the file with
  <title> (2-4 word distinctive name, no dash/colon explainer) then <style>.
- Theme: a motion piece may commit to one visual world (single theme) — then
  paint body background and every color explicitly. Otherwise define tokens on
  :root with dark overrides under @media (prefers-color-scheme: dark)
  :root:not([data-theme="light"]) and :root[data-theme="dark"].
- Must work at phone width (~400 px): the stage scales to fit (aspect-ratio box,
  max-width:100%), no horizontal page scroll, 16 px side gutter.
- Respect prefers-reduced-motion (show a composed static frame or a slow,
  minimal version). Loop seamlessly. A small replay/pause control is welcome.
- The first frame (before/without animation) must already look composed —
  thumbnails capture it.
- Avoid the stock AI looks: cream+serif+terracotta, black+acid green,
  purple-blue gradient, Inter/Space Grotesk, emoji, everything centered. Pick a
  visual identity from the subject's own world (court filings, docket sheets,
  rule citations, deadlines, patent drawings, decision trees, terminal JSON).
  Spend boldness in one place.
- Optional (only if ffmpeg and a headless browser are available locally): also
  record `marketing/engine/litgraph.mp4` (1920x1080, one loop) for social use.
  Don't sink more than ~15 minutes into recording; the HTML is the deliverable.

## Process
Plan (palette 4-6 hex, type roles, storyboard with timings) → build → look at it
rendered ONCE (one headless screenshot is fine) → one pass of fixes → stop.
Commit the result on this task branch (conventional commit, e.g.
`docs(marketing): ...`); do not push, merge, or close anything. Also write
`marketing/engine/NOTES.md`: the file path(s),
the storyboard in 5-8 lines, which real litgraph commands/numbers you used, and
anything you'd polish with more time.

## Your piece: `engine` — WHAT IT'S LIKE TO USE
An engine built for agents and engineers: one JSON request in, one JSON answer
out, with warnings and provenance. Story: `brew install davidawad/tap/litgraph`
typed out → a real request and the real JSON response arriving, highlighting
what matters (policy/value, warnings, provenance) → a fast montage of real
capabilities, each with a real one-line command and a real result fragment:
composing forums (PTAB → Federal Circuit), deadline math with court days and
holidays, sensitivity (tornado), state flags (IPR estoppel), a named scenario,
calibration from published statistics → the same engine as an MCP server so an
AI agent can call it as tools → end card: name, one-line thesis, install line,
GitHub URL, GPL-3.0. Terminal/JSON typography is your bold place — not a
generic hacker terminal; give it an identity from the legal-docket world.
