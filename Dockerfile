# syntax=docker/dockerfile:1

FROM rust:1.95.0-bookworm AS builder

WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        libdbus-1-dev \
        libssl-dev \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock build.rs rust-toolchain.toml ./
COPY src ./src

RUN cargo build --release --locked

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        libdbus-1-3 \
    && rm -rf /var/lib/apt/lists/*

ENV CODEX_HOME=/data/codex

RUN useradd --create-home --home-dir /home/codex --shell /usr/sbin/nologin codex \
    && mkdir -p /data/codex \
    && chown -R codex:codex /data/codex /home/codex

COPY --from=builder /app/target/release/codex-auth-proxy /usr/local/bin/codex-auth-proxy

USER codex
EXPOSE 8765

ENTRYPOINT ["codex-auth-proxy"]
CMD ["--listen", "0.0.0.0:8765", "--device-auth"]
