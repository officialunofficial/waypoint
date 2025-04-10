ARG REGISTRY=localhost
ARG TARGETARCH
# Builder stage
FROM rust:1.85 AS builder
WORKDIR /usr/src/waypoint
RUN apt-get update && apt-get install -y \
    protobuf-compiler \
    libssl-dev \
    pkg-config
COPY Cargo.toml build.rs ./
COPY .sqlx ./.sqlx
COPY src/proto ./src/proto
COPY src ./src
# SQLx offline mode
ARG SQLX_OFFLINE=true
ENV SQLX_OFFLINE=${SQLX_OFFLINE}
# Dummy DATABASE_URL for SQLx, not actually used in offline mode
ENV DATABASE_URL=postgresql://postgres:postgres@localhost:5432/waypoint
RUN cargo build --release

# Runtime stage - minimal image with only necessary runtime dependencies
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl-dev \
    postgresql-client \
    redis-tools \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /usr/src/waypoint/target/release/waypoint /app/
COPY --from=builder /usr/src/waypoint/target/release/backfill /app/
COPY --from=builder /usr/src/waypoint/src/proto /app/proto
RUN chmod +x /app/waypoint /app/backfill
ENV RUST_BACKTRACE=full
# Default command
CMD ["./waypoint", "start"]

# Set metadata labels
LABEL org.opencontainers.image.title="Waypoint"
LABEL org.opencontainers.image.description="Farcaster Hub synchronization tool with streaming and backfill capabilities"
LABEL org.opencontainers.image.source="https://github.com/officialunofficial/waypoint"
LABEL org.opencontainers.image.vendor="Official Unofficial, Inc."