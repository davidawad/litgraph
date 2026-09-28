# litgraph task runner. `just --list` to see all recipes; `just ci` is the
# single entry point CI (and you, locally) should run before landing changes.

set shell := ["bash", "-euo", "pipefail", "-c"]

# Line coverage floor for the pure-logic `litgraph` crate.
cov_floor := "97"

default:
    just --list

# --- setup -----------------------------------------------------------------

# Install the cargo subcommand tools this justfile's recipes call out to.
# Idempotent -- skips anything already on PATH. Prefer `nix develop` or
# `devenv shell` over this where available (pre-built, no compile time).
install:
    command -v cargo-nextest >/dev/null 2>&1 || cargo install cargo-nextest --locked
    command -v cargo-llvm-cov >/dev/null 2>&1 || cargo install cargo-llvm-cov --locked
    command -v cargo-audit >/dev/null 2>&1 || cargo install cargo-audit --locked
    command -v cargo-deny >/dev/null 2>&1 || cargo install cargo-deny --locked
    command -v cargo-machete >/dev/null 2>&1 || cargo install cargo-machete --locked

# Run the CLI (release build) with any extra args, e.g. `just run describe`.
run *ARGS:
    cargo run --release -p litgraph-cli -- {{ARGS}}

# --- formatting & linting -----------------------------------------------

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

lint:
    cargo clippy --workspace --all-targets -- -D warnings

# --- tests ----------------------------------------------------------------

test:
    cargo nextest run --workspace

doctest:
    cargo test --doc --workspace

# proptest-based property tests live under tests/ and src/ like any other
# test and run as part of `test`; nothing extra to invoke here today. This
# recipe exists as the documented hook once dedicated proptest suites land.
proptest:
    cargo nextest run --workspace -E 'test(prop_)'

cov:
    cargo llvm-cov nextest -p litgraph --fail-under-lines {{cov_floor}}

cov-html:
    cargo llvm-cov nextest -p litgraph --html --open

# --- supply chain & static checks -----------------------------------------

audit:
    cargo audit

deny:
    cargo deny check

# Manual lane: unused-dependency check, not part of `ci`.
machete:
    cargo machete

# --- formal verification ---------------------------------------------------

# Kani harnesses live behind #[cfg(kani)] in crates/litgraph. This is
# expected to succeed with zero harnesses (kani exits 0 when no
# #[kani::proof] exists) and start verifying for real the moment the first
# harness is added.
#
# KNOWN GAP (2026-09-28): kani-verifier 0.67.0's bundled nightly is
# rustc 1.93.0-nightly, which is older than this workspace's declared
# `rust-version = "1.97"` (Cargo.toml) -- `cargo kani` refuses to build with
# "rustc 1.93.0-nightly is not supported ... requires rustc 1.97" even with
# zero harnesses. This is an upstream Kani/Rust-release-cadence lag, not
# something fixable from this recipe; re-check after a kani-verifier bump.
verify-kani:
    cargo kani -p litgraph

# --- benchmarking (optional, not part of `ci`) -----------------------------

bench-smoke:
    cargo bench --workspace -- --test

# --- container -------------------------------------------------------------

docker-build:
    docker build -t litgraph:dev .

docker-smoke: docker-build
    docker run --rm litgraph:dev describe | head -20
    echo '{"packs":["cofc","cafc"],"op":{"op":"solve"}}' | docker run --rm -i litgraph:dev q -

# --- wasm --------------------------------------------------------------

# Build litgraph-wasm for the browser and Node targets into dist/wasm/.
# Packs are embedded in the wasm binary (same build.rs as the CLI), so no
# LITGRAPH_PACKS/packs-dir setup is needed at runtime.
wasm:
    cargo build --release -p litgraph-wasm --target wasm32-unknown-unknown
    mkdir -p dist/wasm/web dist/wasm/node
    wasm-bindgen --target web --out-dir dist/wasm/web target/wasm32-unknown-unknown/release/litgraph_wasm.wasm
    wasm-bindgen --target nodejs --out-dir dist/wasm/node target/wasm32-unknown-unknown/release/litgraph_wasm.wasm

# KNOWN GAP (2026-09-28, verified locally with wasm-bindgen-cli 0.2.127):
# this currently panics -- crates/litgraph/src/api/mod.rs:143 calls
# std::time::Instant::now() (elapsed_ms timing), which has no clock source
# on wasm32-unknown-unknown and traps with "RuntimeError: unreachable".
# Out of scope for this packaging workstream (crates/** owned elsewhere);
# needs a #[cfg(target_arch = "wasm32")] time source (e.g. js_sys::Date).
wasm-smoke: wasm
    node -e "const m=require('./dist/wasm/node/litgraph_wasm.js'); const r=JSON.parse(m.handle(JSON.stringify({packs:['cofc'],op:{op:'solve'}}))); if(!r.ok){console.error(r);process.exit(1)} console.log('wasm smoke ok')"

# --- nix ---------------------------------------------------------------

nix-build:
    nix build .#default
    ./result/bin/litgraph describe | head -5

nix-check:
    nix flake check

# --- aggregate --------------------------------------------------------------

# The single CI entry point: fast, deterministic gates only. Coverage,
# audit, and deny hit the network/build cache so they're last.
ci: fmt-check lint test doctest cov audit deny
    @echo "ci: all gates passed"

clean:
    cargo clean
    rm -r -f dist result
