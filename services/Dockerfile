# The Rust services (api, chat, obs-link, db-migrator), one image each from
# this file. Build context: the repository root.
#
#   docker build -f services/Dockerfile --build-arg SERVICE=api .

# The latest stable Rust, like CI and the flake. The Debian release is named
# so the builder's glibc is the runtime's.
FROM rust:1-slim-trixie AS chef
RUN cargo install --locked cargo-chef
WORKDIR /app

FROM chef AS planner
ARG SERVICE
COPY . .
RUN cargo chef prepare --recipe-path recipe.json --bin "${SERVICE:?pass --build-arg SERVICE=<service>}"

FROM chef AS builder
ARG SERVICE
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json --bin "$SERVICE"

COPY . .
# The commit, as service.version in telemetry (after the cook, so it keeps
# the dependency cache).
ARG GIT_SHA
RUN cargo build --release --locked -p "$SERVICE"

FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd -g 1001 appuser \
    && useradd -r -u 1001 -g appuser -s /usr/sbin/nologin appuser

ARG SERVICE
COPY --from=builder /app/target/release/$SERVICE /usr/local/bin/$SERVICE

USER appuser
# An optional config.toml is read from here.
WORKDIR /app

ENV RUST_LOG=info \
    SERVICE=$SERVICE

# Through a shell, since CMD can't take the build argument; `exec` leaves the
# service as PID 1, so it gets the stop signal.
CMD ["/bin/sh", "-c", "exec \"$SERVICE\""]
