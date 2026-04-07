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
# Builder
# ---------------------------------------------------------------------------
FROM rust:1.85-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY . .

ARG BACKEND
RUN cargo build --release -p fileloft-server \
        --no-default-features --features "backend-${BACKEND}"

# ---------------------------------------------------------------------------
# Runtime
# ---------------------------------------------------------------------------
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/fileloft-server /usr/local/bin/fileloft-server

RUN mkdir -p /var/lib/fileloft

ENV FILELOFT_BIND=0.0.0.0:8080
ENV FILELOFT_DATA_DIR=/var/lib/fileloft
ENV FILELOFT_BASE_PATH=/files/

EXPOSE 8080
ENTRYPOINT ["fileloft-server"]
