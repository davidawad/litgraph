# Packaging and release pitfalls (2026-09-29)

Durable facts learned while adding scenarios, calibration, cite sources,
and automatic releases. Each one cost a failed build or a silently wrong
artifact.

## Every packaging context must carry every embedded data directory

**Problem.** `crates/litgraph/build.rs` embeds `packs/`, `scenarios/`,
`calibration/` and `sources/`. The nix flake's crane source filter and the
Dockerfile each copied only `packs/`, so those builds embedded an empty
scenario library and no sources. Nothing failed until a nix test happened to
use a named scenario. The Docker image shipped without them.

**Root cause.** `embed_dir` treated a missing directory as empty.

**Durable fact.** `build.rs` now panics when a data directory is missing
(1577a5a), so a packaging context that forgets one fails to build. When you
add a data directory, add it to `flake.nix` `dataDirs` and to the
Dockerfile's `COPY` lines. The build will tell you if you don't.

## Line-based merges can produce valid-looking JSON with duplicate keys

**Problem.** Two branches each added a top-level `"sources"` array to the
same pack at different positions. Git merged them with no conflict marker.
Generic JSON parsers accepted the file, but serde's derived `Deserialize`
for `Pack` rejects duplicate fields, so every test that loaded the embedded
catalog failed.

**Durable fact.** After merging pack JSON, parse it with duplicate-key
detection (Python `json.load(..., object_pairs_hook=...)` or the strict
`Pack` struct), not a lenient parser. `Catalog::from_files` now skips and
reports a malformed pack (`describe` shows `pack_load_errors`) instead of
failing every request, and
`every_embedded_pack_file_parses_individually_against_the_strict_pack_struct`
names the bad file.

## A tag pushed with GITHUB_TOKEN does not trigger other workflows

**Durable fact.** GitHub does not start workflows for events created with
the job's `GITHUB_TOKEN`. `auto-release.yml` therefore calls `release.yml`
as a reusable workflow (`workflow_call` with a `tag` input) instead of
relying on the tag push. `release.yml` reads the tag from
`inputs.tag || github.ref_name`, so a hand-pushed tag still works.

## The reusable release call must declare read-only cache access

**Durable fact.** CI's actionlint (`reviewdog/action-actionlint`) enforces
`cache-call-unrestricted`: a reusable-workflow call from a low-trust trigger
such as `workflow_run` needs an explicit `cache-mode`. The `release` job in
`auto-release.yml` sets `cache-mode: read`, and docker only exports its
build cache on a hand-pushed tag. Upstream actionlint 1.7.12 does not know
the `cache-mode` key yet, so a local run reports it as unexpected. CI is
authoritative until upstream catches up. See the
[GitHub changelog](https://github.blog/changelog/2026-09-10-control-github-actions-cache-access-with-cache-mode/).

## The pinned toolchain overrides the toolchain action's targets

**Durable fact.** `rust-toolchain.toml` pins the channel, so
`dtolnay/rust-toolchain`'s `targets:` input installs targets onto a
toolchain the build never uses. Release jobs run
`rustup target add <target>` explicitly after checkout.
