# Thin delegation to justfile (source of truth for recipes) -- this file
# exists because swe-repo-standard tooling and some CI templates expect a
# Makefile with these target names. See ./justfile for the real recipes.

.PHONY: install lint test run ci clean

install:
	@command -v cargo >/dev/null || { echo "install rustup: https://rustup.rs"; exit 1; }
	@command -v just >/dev/null || cargo install just
	@command -v cargo-nextest >/dev/null || cargo install cargo-nextest --locked
	@command -v cargo-llvm-cov >/dev/null || cargo install cargo-llvm-cov --locked
	@command -v cargo-audit >/dev/null || cargo install cargo-audit --locked
	@command -v cargo-deny >/dev/null || cargo install cargo-deny --locked
	@command -v cargo-machete >/dev/null || cargo install cargo-machete --locked

lint:
	just lint

test:
	just test

run:
	just run

ci:
	just ci

clean:
	just clean
