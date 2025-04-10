# Waypoint Project Summary

## Overview
Waypoint is a Snapchain synchronization tool built in Rust, optimized for memory efficiency. It provides a streaming synchronization service combined with backfill capabilities to process historical data.

## Key Components

### Main Service
- **Streaming Service**: Subscribes to real-time Snapchain events and processes them
- **Commands**: Run with `./waypoint start` or `make run`

### Backfill System
- **Worker Containers**: Process historical data from Snapchain
- **Queue**: Redis-based job queue for coordinating work
- **Commands**: 
  - Queue: `make backfill-queue`
  - Workers: `make backfill-worker`
  - High-performance Workers: `make backfill-worker-highperf` (100x faster)

## Docker Development

```bash
# Use docker-compose for local development with PostgreSQL 17 + pgvector
docker compose up

# Build Docker image
make docker-build

# Run the Docker container
make docker-run
```

## Common Commands

### Local Development
```bash
# Build the project
make build

# Run the main service
make run

# Queue backfill jobs
make backfill-queue                    # Queue all FIDs
make backfill-queue-fids FIDS=1,2,3    # Queue specific FIDs
make backfill-queue-max MAX_FID=1000   # Queue FIDs up to 1000

# Run a backfill worker
make backfill-worker                   # Standard worker (50 concurrent jobs)
make backfill-worker-highperf          # High-performance worker (100 concurrent jobs)

# Update user_data
make backfill-update-user-data         # Update user_data for all FIDs
make backfill-update-user-data-max MAX_FID=1000  # Update user_data for FIDs up to 1000
```

### Docker & Deployment
```bash
# Build Docker image
make docker-build

# Push to registry
make docker-push

# Build and push with a specific tag
make docker-tag TAG=v1.0.0
```

## Architecture

The architecture is documented in detail in [ARCHITECTURE.md](ARCHITECTURE.md). Key features:

- **Zero-copy gRPC**: Optimized Snapchain event processing
- **Redis Streams**: Message queuing to prevent OOM errors
- **Batch Processing**: Cursor-based pagination for memory efficiency
- **Backpressure Handling**: Configurable memory limits
- **Static Buffers**: Minimal heap allocations

## Environment Variables
```
# Connection strings (these can use WAYPOINT_ prefix for config system)
WAYPOINT_DATABASE__URL=postgresql://postgres:postgres@postgres:5432/waypoint
WAYPOINT_DATABASE__MAX_CONNECTIONS=20
WAYPOINT_DATABASE__TIMEOUT_SECONDS=60
WAYPOINT_REDIS__URL=redis://redis:6379
WAYPOINT_HUB__URL=snapchain.farcaster.xyz:3383
RUST_LOG=info

# Backfill performance tuning
BACKFILL_CONCURRENCY=50  # Number of concurrent FIDs to process
```

You can also use a configuration file:
```bash
# Run with custom configuration file
WAYPOINT_CONFIG=config/sample.toml make run
```

## Backfill Optimizations

The backfill system has been optimized for high throughput:

1. **Concurrency**:
   - Configurable concurrency via `BACKFILL_CONCURRENCY` environment variable (default: 50)
   - Each worker can process up to 100 concurrent FIDs
   - Multiple workers can run in parallel across multiple containers

2. **Batch Processing**:
   - Increased job batch size from 10 to 50 FIDs per job
   - Increased API page size from 100 to 1000 messages per request
   - Message chunking with semaphore-controlled concurrency

3. **Parallel Processing**:
   - Parallel fetching of different message types (casts, reactions, etc.)
   - Concurrent processing of messages within a single FID
   - Batched database operations for better throughput

4. **Performance Tuning**:
   - Release mode compilation for production
   - Configurable logging levels for reduced overhead
   - Adaptive rate limiting based on Snapchain server response

## Key Files
- `src/main.rs`: Main application entry point
- `src/bin/backfill.rs`: Backfill worker and queue code
- `src/backfill/reconciler.rs`: Message reconciliation logic
- `src/backfill/worker.rs`: Worker implementation
- `Dockerfile`: Container build instructions
- `docker-compose.yml`: Local development setup with PostgreSQL 17 and pgvector
- `ARCHITECTURE.md`: Detailed system architecture documentation