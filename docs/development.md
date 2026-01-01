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
3. Create the database:
   ```bash
   # Create the database
   createdb waypoint
   ```

**Note:** Database migrations are now run automatically when Waypoint starts. The migrations are embedded in the binary and will be applied to your database on first run. You don't need to run them manually unless you're developing new migrations.

### Database Migrations

Waypoint uses SQLx for database operations with embedded migrations. Migrations are automatically run when the application starts.

**Important:** Migrations are embedded in the Waypoint binary at compile time, so they work out of the box with Docker images without requiring the migrations directory to be present.

When adding new migrations for development:

1. Create a new migration file in the `migrations/` directory with the format `NNN_description.sql`
2. (Optional) Apply the migration to your local database manually for testing:
   ```bash
   psql -U postgres -d waypoint -f migrations/NNN_description.sql
   ```
3. If using SQLx compile-time checking, ensure `DATABASE_URL` is set:
   ```bash
   export DATABASE_URL=postgresql://postgres:postgres@localhost:5432/waypoint
   ```
4. Run `cargo sqlx prepare` to update the `.sqlx` directory with query metadata
5. Commit both the migration file and any `.sqlx` changes

#### Current Migrations

- `001_init.sql` - Initial database schema with all core tables
- `002_add_tier_purchases.sql` - Adds support for Farcaster Pro tier purchases

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

Waypoint includes a powerful backfill system for processing historical data using a queue/worker architecture.

### Local Development (make commands)

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

### Docker Compose Backfill

For production-like backfills using Docker Compose:

```bash
# Start backfill with default single worker
docker compose --profile backfill up

# Scale to multiple workers for faster processing
docker compose --profile backfill up --scale backfill-worker=4

# Or use dedicated backfill compose file
docker compose -f docker-compose.backfill.yml up

# Scale workers via environment variable
BACKFILL_WORKERS=4 docker compose -f docker-compose.backfill.yml up
```

### Worker CLI Options

The backfill worker supports these command-line options:

| Option | Description | Default |
|--------|-------------|---------|
| `--exit-on-complete` | Exit when queue is empty (instead of running forever) | false |
| `--idle-timeout <secs>` | Seconds to wait with empty queue before exiting | 30 |

Example:
```bash
./waypoint backfill fid worker --exit-on-complete --idle-timeout 60
```

### How It Works

1. **Queue Service**: Connects to the hub, fetches all FIDs, and populates Redis with batched jobs
2. **Worker Service**: Pulls jobs from Redis, reconciles messages from the hub, and stores in PostgreSQL
3. **Scaling**: Multiple workers can process the same Redis queue concurrently (BRPOP is atomic)
4. **Completion**: Workers exit automatically when idle for the configured timeout

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
BACKFILL_CONCURRENCY=4  # Number of concurrent FIDs to process (max: 8)
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