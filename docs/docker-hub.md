# Waypoint: Snapchain Synchronization Tool

Waypoint is a Snapchain synchronization tool built in Rust, optimized for memory efficiency. It provides streaming synchronization for real-time Snapchain events alongside a Redis-based backfill system for historical data processing.

## Features

- Real-time streaming service that subscribes to Snapchain events
- Redis-based backfill system for processing historical data
- Memory-efficient architecture with optimized gRPC processing
- High-performance concurrent processing
- Kubernetes-ready with deployment configurations

## Quick Start

```bash
# Run with Docker Compose (includes PostgreSQL 17 with pgvector and Redis 7)
docker compose up

# Run standalone container (you'll need to provide connection details)
docker run -e WAYPOINT_DATABASE__URL=postgresql://user:pass@host:5432/waypoint \
  -e WAYPOINT_DATABASE__MAX_CONNECTIONS=20 \
  -e WAYPOINT_DATABASE__TIMEOUT_SECONDS=60 \
  -e WAYPOINT_DATABASE__STORE_RAW_MESSAGES=true \
  -e WAYPOINT_REDIS__URL=redis://host:6379 \
  -e WAYPOINT_HUB__URL=snapchain.farcaster.xyz:3383 \
  -e HOST=0.0.0.0 \
  -e PORT=8080 \
  -e RUST_LOG=info \
  -p 8080:8080 \
  officialunofficial/waypoint:latest
```

## Configuration Options

Waypoint can be configured using environment variables with the `WAYPOINT_` prefix:

```
# Connection strings and database configuration
WAYPOINT_DATABASE__URL=postgresql://postgres:postgres@postgres:5432/waypoint
WAYPOINT_DATABASE__MAX_CONNECTIONS=20
WAYPOINT_DATABASE__TIMEOUT_SECONDS=60
WAYPOINT_REDIS__URL=redis://redis:6379
WAYPOINT_HUB__URL=snapchain.farcaster.xyz:3383

# Server configuration
HOST=0.0.0.0
PORT=8080
RUST_LOG=info

# Backfill performance tuning
BACKFILL_CONCURRENCY=4  # Number of concurrent FIDs to process (max: 8)
```

## Supported Tags

- `latest`: Latest stable release
- `x.y.z`: Specific version releases
- `edge`: Latest build from main branch

## Source Code

GitHub repository: [https://github.com/officialunofficial/waypoint](https://github.com/officialunofficial/waypoint)

