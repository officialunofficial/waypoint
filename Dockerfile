ARG TARGETARCH
ARG REGISTRY=docker.io

# Builder stage - use bookworm (Debian 12) for GLIBC compatibility
FROM rust:1.90-bookworm AS builder
ARG TARGETARCH
WORKDIR /usr/src/waypoint
# Install base dependencies and cross-compilation toolchains for ARM64
RUN apt-get update && apt-get install -y \
    protobuf-compiler \
    libssl-dev \
    pkg-config \
    gcc-aarch64-linux-gnu \
    g++-aarch64-linux-gnu \
    && rm -rf /var/lib/apt/lists/*
# Configure Rust for ARM64 cross-compilation if needed
RUN if [ "$TARGETARCH" = "arm64" ]; then \
    rustup target add aarch64-unknown-linux-gnu && \
    mkdir -p /usr/local/cargo && \
    echo '[target.aarch64-unknown-linux-gnu]' > /usr/local/cargo/config.toml && \
    echo 'linker = "aarch64-linux-gnu-gcc"' >> /usr/local/cargo/config.toml; \
    fi
COPY Cargo.toml build.rs ./
COPY .sqlx ./.sqlx
COPY src/proto ./src/proto
COPY src ./src
# SQLx offline mode
ARG SQLX_OFFLINE=true
ENV SQLX_OFFLINE=${SQLX_OFFLINE}
# Dummy DATABASE_URL for SQLx, not actually used in offline mode
ENV DATABASE_URL=postgresql://postgres:postgres@localhost:5432/waypoint
# Build for target architecture (native for amd64, cross-compile for arm64)
# Then copy to a consistent location for easier COPY in runtime stage
RUN if [ "$TARGETARCH" = "arm64" ]; then \
    cargo build --release --target aarch64-unknown-linux-gnu && \
    cp target/aarch64-unknown-linux-gnu/release/waypoint target/waypoint; \
    else \
    cargo build --release && \
    cp target/release/waypoint target/waypoint; \
    fi

# Runtime stage - use Debian 12 to match builder GLIBC version
FROM debian:12-slim
ARG TARGETARCH
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl-dev \
    postgresql-client \
    redis-tools \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
# Copy binary from consistent location
COPY --from=builder /usr/src/waypoint/target/waypoint /app/waypoint
COPY --from=builder /usr/src/waypoint/src/proto /app/proto
RUN chmod +x /app/waypoint
ENV RUST_BACKTRACE=full
# Default command
CMD ["./waypoint", "start"]

# Set metadata labels
LABEL org.opencontainers.image.title="Waypoint"
LABEL org.opencontainers.image.description="Farcaster Hub synchronization tool with streaming and backfill capabilities"
LABEL org.opencontainers.image.source="https://github.com/officialunofficial/waypoint"
LABEL org.opencontainers.image.vendor="Official Unofficial, Inc."
