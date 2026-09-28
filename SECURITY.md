# Security Policy

## Reporting a Vulnerability

Please **do not** open a public GitHub issue for security vulnerabilities.

Report privately via [GitHub Security Advisories](https://github.com/davidawad/litgraph/security/advisories/new)
for this repository. This lets us discuss, fix, and coordinate disclosure
before the report becomes public.

Include, where possible:

- A description of the vulnerability and its potential impact.
- Steps to reproduce (a minimal request/pack that triggers it is ideal —
  litgraph's engine takes untrusted JSON requests and pack files as input).
- The affected version/commit.

We'll acknowledge reports as promptly as we can, work with you on a fix, and
credit you in the advisory unless you'd prefer otherwise.

## Scope

litgraph is a procedural-modeling and analysis engine (expression evaluator,
graph solver, JSON CLI/library). Security-relevant areas include:

- The `expr` expression language (parser/evaluator) — anything that could let
  an untrusted `scenario` expression escape evaluation, panic in an unsafe
  way, or exhaust resources (unbounded recursion/memory) deserves a report.
- Pack/request JSON parsing — malformed or adversarial input that causes a
  crash, hang, or memory issue rather than a clean `{ok:false, error}`.
- Any dependency with a known vulnerability affecting a shipped binary.

litgraph does not execute arbitrary code, make network calls, or touch the
filesystem beyond reading pack files and CLI-specified paths — so most
"security" issues here are robustness/DoS issues in the expression evaluator
or solver, not memory-safety-in-the-traditional-sense (this is Rust) or
remote code execution. Still worth a private report.

## Not a Legal-Advice Concern

litgraph's outputs (probabilities, costs, payoffs) are estimates for
teaching/tooling purposes, not legal advice — see the disclaimer in
[README.md](README.md). Disagreement with a modeled outcome or a pack's
content is not a security report; open a regular issue or PR instead.
