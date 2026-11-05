# syntax=docker/dockerfile:1
# Images for the FUber API, worker and migration job (TRD §2.3). Build from backend/:
#
#   docker build --target api     -t fuber-api .
#   docker build --target worker  -t fuber-worker .
#   docker build --target migrate -t fuber-migrate .
#
# Nothing secret is baked in: configuration comes from the environment (Secret Manager on
# Google Cloud). Runtime images are distroless (no shell, no package manager) and run as the
# unprivileged `nonroot` user.

ARG RUST_VERSION=1

FROM rust:${RUST_VERSION}-slim-bookworm AS chef
RUN cargo install cargo-chef --locked
WORKDIR /src

# Dependency recipe only, so the dependency layer is cached until Cargo.toml/Cargo.lock change.
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /src/recipe.json recipe.json
RUN cargo chef cook --release --locked --recipe-path recipe.json
COPY . .
RUN cargo build --release --locked --bin fuber-api --bin fuber-worker --bin migrate

FROM gcr.io/distroless/cc-debian12:nonroot AS runtime
USER nonroot:nonroot
ENV RUST_LOG=info

FROM runtime AS api
COPY --from=builder /src/target/release/fuber-api /usr/local/bin/fuber-api
# Cloud Run sends traffic to 8080; the API binds 0.0.0.0:8080 unless BIND_ADDR says otherwise.
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/fuber-api"]

FROM runtime AS worker
COPY --from=builder /src/target/release/fuber-worker /usr/local/bin/fuber-worker
ENTRYPOINT ["/usr/local/bin/fuber-worker"]

FROM runtime AS migrate
COPY --from=builder /src/target/release/migrate /usr/local/bin/migrate
ENTRYPOINT ["/usr/local/bin/migrate"]
