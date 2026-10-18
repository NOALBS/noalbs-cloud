# Build stage
FROM rust:1.97-slim AS chef
RUN cargo install cargo-chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json --bin db-migrator

FROM chef AS builder
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libpq-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json --bin db-migrator

COPY . .
RUN cargo build --release -p db-migrator

# Runtime stage
FROM debian:trixie-slim
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd -g 1001 appuser && \
    useradd -r -u 1001 -g appuser -s /usr/sbin/nologin appuser

RUN mkdir -p /app /app/logs /app/config && chown -R appuser:appuser /app
COPY --from=builder /app/target/release/db-migrator /usr/local/bin/db-migrator
RUN chown appuser:appuser /usr/local/bin/db-migrator

USER appuser
WORKDIR /app

ENV RUST_LOG=info

CMD ["db-migrator"]