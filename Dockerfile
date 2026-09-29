# syntax=docker/dockerfile:1

# litgraph — litigation procedure graphs as stochastic games.
# Multi-stage build: cargo-chef caches dependency compilation separately
# from source changes, then a minimal non-root runtime image ships just
# the binary + packs.

ARG RUST_VERSION=1.97.1

# --- planner: compute the cargo-chef recipe (dependency graph only) --------
FROM rust:${RUST_VERSION}-slim-bookworm AS chef
WORKDIR /build
RUN cargo install cargo-chef --locked
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY crates crates
RUN cargo chef prepare --recipe-path recipe.json

# --- builder: build deps from the recipe (cached), then the real source ----
FROM chef AS builder
COPY --from=planner /build/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY packs packs
RUN cargo build --release -p litgraph-cli && \
    strip target/release/litgraph

# --- runtime: minimal non-root image ---------------------------------------
FROM debian:bookworm-slim AS runtime

ARG BUILD_DATE
ARG VCS_REF

LABEL org.opencontainers.image.title="litgraph" \
      org.opencontainers.image.description="Litigation procedure graphs as stochastic games: load, compose, weight, solve, simulate." \
      org.opencontainers.image.source="https://github.com/davidawad/litgraph" \
      org.opencontainers.image.licenses="GPL-3.0-or-later" \
      org.opencontainers.image.created="${BUILD_DATE}" \
      org.opencontainers.image.revision="${VCS_REF}"

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/* && \
    useradd --system --no-create-home --uid 10001 litgraph

COPY --from=builder /build/target/release/litgraph /usr/local/bin/litgraph
COPY --from=builder /build/packs /usr/share/litgraph/packs

ENV LITGRAPH_PACKS=/usr/share/litgraph/packs
USER litgraph
WORKDIR /home/litgraph

ENTRYPOINT ["litgraph"]
CMD ["describe"]
