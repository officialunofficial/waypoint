# Docker Hub

Snapchain sync engine for Farcaster. Streams events and backfills historical data to PostgreSQL.

## Quick Start

```bash
docker compose up
```

## Standalone

```bash
docker run -p 8080:8080 \
  -e WAYPOINT_DATABASE__URL=postgresql://user:pass@host:5432/waypoint \
  -e WAYPOINT_REDIS__URL=redis://host:6379 \
  -e WAYPOINT_HUB__URL=snapchain.farcaster.xyz:3383 \
  officialunofficial/waypoint:latest
```

## Backfill

```bash
docker compose --profile backfill up --scale backfill-worker=4
```

## Tags

- `latest` - stable release
- `x.y.z` - version
- `edge` - main branch

## Source

https://github.com/officialunofficial/waypoint
