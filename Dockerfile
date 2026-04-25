# Multi-stage Dockerfile for fileloft.
#
# Build-arg BACKEND selects the storage backend (fs, s3, gcs, azure).
# Default: fs (filesystem).
#
# Examples:
#   docker build -t fileloft:latest .
#   docker build --build-arg BACKEND=s3  -t fileloft:s3  .
#   docker build --build-arg BACKEND=gcs -t fileloft:gcs .

ARG BACKEND=fs

# ---------------------------------------------------------------------------
# Base with cargo-chef
# ---------------------------------------------------------------------------
FROM lukemathwalker/cargo-chef:latest-rust-1.95-slim-trixie AS chef

WORKDIR /build

# ---------------------------------------------------------------------------
# Dependency planning
# ---------------------------------------------------------------------------
FROM chef AS planner

COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ---------------------------------------------------------------------------
# Builder
# ---------------------------------------------------------------------------
FROM chef AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

ARG BACKEND
COPY --from=planner /build/recipe.json recipe.json

RUN cargo chef cook --release --recipe-path recipe.json \
        -p fileloft-server --no-default-features --features "backend-${BACKEND}"

COPY . .
RUN cargo build --release -p fileloft-server \
        --no-default-features --features "backend-${BACKEND}"

# ---------------------------------------------------------------------------
# Runtime
# ---------------------------------------------------------------------------
FROM debian:trixie-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/fileloft-server /usr/local/bin/fileloft-server

RUN groupadd --system fileloft \
    && useradd --system --gid fileloft --home-dir /var/lib/fileloft --shell /usr/sbin/nologin fileloft \
    && mkdir -p /var/lib/fileloft \
    && chown -R fileloft:fileloft /var/lib/fileloft

ENV FILELOFT_BIND=0.0.0.0:8080
ENV FILELOFT_DATA_DIR=/var/lib/fileloft
ENV FILELOFT_BASE_PATH=/files/

EXPOSE 8080
USER fileloft:fileloft
ENTRYPOINT ["fileloft-server"]
