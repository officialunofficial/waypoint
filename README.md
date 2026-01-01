# Waypoint

[![Docker Hub](https://img.shields.io/docker/pulls/officialunofficial/waypoint)](https://hub.docker.com/r/officialunofficial/waypoint)

![waypoint banner](banner.png)

Rust-based Snapchain sync engine. Streams real-time events and backfills historical data to PostgreSQL.

## Quick Start

```bash
# Docker (recommended)
docker compose up

# Local development
make env-setup    # creates .env from example
make build
make run
```

## Backfill

Backfill uses a queue/worker model. The queue service populates Redis with FID batches, workers process them concurrently.

```bash
# Docker
docker compose --profile backfill up
docker compose --profile backfill up --scale backfill-worker=4  # parallel workers

# Local
make backfill-queue
make backfill-worker
```

Workers exit automatically when the queue is empty for 60s.

## Configuration

Copy `.env.example` to `.env` and edit. Key settings:

```bash
WAYPOINT_DATABASE__URL=postgresql://postgres:postgres@localhost:5432/waypoint
WAYPOINT_REDIS__URL=redis://localhost:6379
WAYPOINT_HUB__URL=snapchain.farcaster.xyz:3383
```

See `.env.example` for all options including MCP, metrics, and Ethereum settings.

### Docker networking

Never use `localhost` for hub URLs in containers. Use:
- `snapchain:3381` (compose network)
- `host.docker.internal:3381` (Docker Desktop)
- `172.17.0.1:3381` (Linux bridge)

## Docs

- [Development Guide](docs/development.md)
- [Architecture](docs/architecture.md)
- [MCP Service](docs/mcp.md)
- [Metrics](docs/metrics.md)
- [Changelog](docs/changelog.md)
- [Contributing](docs/contributing.md)
