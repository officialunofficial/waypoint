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

### MCP Service
- **Model Context Protocol**: Provides AI assistants with access to Farcaster data
- **Available Tools**: Fetch user profiles, verifications, casts, and replies
- **Documentation**: See [mcp.md](mcp.md) for details

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

# Queue FID-based backfill jobs
make backfill-queue                    # Queue all FIDs
make backfill-queue-fids FIDS=1,2,3    # Queue specific FIDs
make backfill-queue-max MAX_FID=1000   # Queue FIDs up to 1000

# Run a FID-based backfill worker
make backfill-worker                   # Run backfill worker (50 concurrent jobs by default)

# Update user_data
make backfill-update-user-data         # Update user_data for all FIDs
make backfill-update-user-data-max MAX_FID=1000  # Update user_data for FIDs up to 1000

# Metrics & Monitoring
make metrics-start                     # Start metrics infrastructure (Grafana + StatsD)
make metrics-stop                      # Stop metrics infrastructure
make metrics-open                      # Open Grafana dashboard in browser
./run-with-metrics.sh <command>        # Run any command with metrics enabled
```

### Running with Metrics

```bash
# Start metrics infrastructure
make metrics-start

# Run backfill with metrics enabled (shows progress in Grafana)
./run-with-metrics.sh make backfill-worker

# Queue jobs with metrics
./run-with-metrics.sh make backfill-queue-fids FIDS=1,2,3

# Open Grafana dashboard to view metrics
make metrics-open
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

### Metrics
```bash
# Start the metrics infrastructure
make metrics-start

# Run any command with metrics enabled
./run-with-metrics.sh make backfill-worker

# Open Grafana dashboard in browser
make metrics-open

# Stop the metrics infrastructure when done
make metrics-stop
```

## Architecture

The architecture is documented in detail in [ARCHITECTURE.md](ARCHITECTURE.md). Key features:

- **Zero-copy gRPC**: Optimized Snapchain event processing
- **Redis Streams**: Message queuing to prevent OOM errors
- **Batch Processing**: Cursor-based pagination for memory efficiency
- **Backpressure Handling**: Configurable memory limits
- **Static Buffers**: Minimal heap allocations
- **StatsD Metrics**: Real-time performance tracking with Grafana visualization

## Environment Variables
```
# Connection strings (these can use WAYPOINT_ prefix for config system)
WAYPOINT_DATABASE__URL=postgresql://postgres:postgres@postgres:5432/waypoint
WAYPOINT_DATABASE__MAX_CONNECTIONS=20
WAYPOINT_DATABASE__TIMEOUT_SECONDS=60
WAYPOINT_REDIS__URL=redis://redis:6379
WAYPOINT_HUB__URL=snapchain.farcaster.xyz:3383

# MCP service configuration
WAYPOINT_MCP__ENABLED=true
WAYPOINT_MCP__BIND_ADDRESS=127.0.0.1  # Use 0.0.0.0 to allow external connections
WAYPOINT_MCP__PORT=8000

RUST_LOG=info

# Backfill performance tuning
BACKFILL_CONCURRENCY=50  # Number of concurrent FIDs to process

# Metrics configuration
WAYPOINT_STATSD__ENABLED=true          # Enable/disable metrics collection
WAYPOINT_STATSD__ADDR=localhost:8125   # StatsD server address (host:port)
WAYPOINT_STATSD__PREFIX=way_read      # Prefix for all metrics
WAYPOINT_STATSD__USE_TAGS=false        # Whether to use tagged metrics
```

You can also use a configuration file:
```bash
# Run with custom configuration file
WAYPOINT_CONFIG=config/sample.toml make run
```

## Backfill Optimizations

The backfill system has been optimized for high throughput with two complementary approaches:

### FID-based Backfill

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

### General Performance Tuning

- Release mode compilation for production
- Configurable logging levels for reduced overhead
- Adaptive rate limiting based on Snapchain server response

## Key Files
- `src/main.rs`: Main application entry point with unified CLI commands
- `src/backfill/reconciler.rs`: FID-based reconciliation logic
- `src/backfill/worker.rs`: FID-based worker implementation
- `src/services/mcp.rs`: MCP service implementation for AI tools
- `src/metrics.rs`: Metrics collection and monitoring
- `Dockerfile`: Container build instructions
- `docker-compose.yml`: Local development setup with PostgreSQL 17, pgvector, Graphite and Grafana
- `METRICS.md`: Detailed metrics system documentation
- `mcp.md`: MCP service documentation
- `grafana/`: Grafana dashboards and configuration
- `ARCHITECTURE.md`: Detailed system architecture documentation