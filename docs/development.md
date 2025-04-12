# Development Guide

This guide covers local development of Waypoint.

## Prerequisites

- [Rust](https://www.rust-lang.org/) 1.85 or later
- [Docker](https://www.docker.com/) and Docker Compose
- PostgreSQL 17+ with pgvector (or use the Docker setup)

## Setting Up a Development Environment

### Using Docker Compose (Recommended)

The easiest way to set up a development environment is to use Docker Compose:

```bash
# Start PostgreSQL, Redis, and other dependencies
docker compose up -d
```

### Manual Setup

If you prefer to run services manually:

1. Install PostgreSQL 17+ with pgvector extension
2. Install Redis 7+
3. Run the database initialization script:
   ```bash
   psql -U postgres -d waypoint -f migrations/init.sql
   ```

## Building

```bash
# Build the project
make build

# Build with optimizations for production
cargo build --release
```

## Local Development

```bash
# Run the main service
make run

# Use a custom configuration file
WAYPOINT_CONFIG=config/sample.toml make run
```

## Backfill System

Waypoint includes a powerful backfill system for processing historical data:

```bash
# Queue backfill jobs
make backfill-queue                    # Queue all FIDs
make backfill-queue-fids FIDS=1,2,3    # Queue specific FIDs
make backfill-queue-max MAX_FID=1000   # Queue FIDs up to 1000

# Run a backfill worker
make backfill-worker                   # Run backfill worker

# Update user_data
make backfill-update-user-data         # Update user_data for all FIDs
make backfill-update-user-data-max MAX_FID=1000  # Update user_data for FIDs up to 1000
```

## Metrics

Waypoint includes a metrics system based on StatsD/Graphite with Grafana visualization.

```bash
# Start the metrics infrastructure
make metrics-start

# Run a command with metrics enabled
./run-with-metrics.sh make backfill-worker

# Open Grafana dashboard in browser
make metrics-open

# Stop the metrics infrastructure
make metrics-stop
```

See [metrics-setup.md](metrics-setup.md) and [metrics.md](metrics.md) for detailed information about the metrics system.

## Testing

```bash
# Run all tests
make test

# Run a specific test
cargo test test_name
```

## Docker Development

```bash
# Build Docker image
make docker-build

# Run the Docker container
make docker-run

# Build and tag with a specific version
make docker-tag TAG=v1.0.0
```

## Environment Variables

Waypoint uses a flexible configuration system based on environment variables:

```
# Database
WAYPOINT_DATABASE__URL=postgresql://postgres:postgres@localhost:5432/waypoint
WAYPOINT_DATABASE__MAX_CONNECTIONS=20

# Redis
WAYPOINT_REDIS__URL=redis://localhost:6379

# Hub connection
WAYPOINT_HUB__URL=snapchain.farcaster.xyz:3383

# Logging
RUST_LOG=info

# Backfill performance
BACKFILL_CONCURRENCY=50  # Number of concurrent FIDs to process
```

## Formatting and Linting

```bash
# Format Rust code
make fmt-rust

# Format all code
make fmt
```

## Creating Documentation

The documentation for Waypoint is written in Markdown and stored in the `docs/` directory:

- `README.md` - Main project documentation
- `ARCHITECTURE.md` - Detailed system architecture
- `METRICS.md` - Comprehensive metrics documentation
- `development.md` - This development guide
- `metrics-setup.md` - Guide for setting up metrics