# Waypoint

Waypoint is a Snapchain synchronization tool built in Rust, optimized for memory efficiency. It provides a streaming synchronization service combined with backfill capabilities to process historical data.

[![Docker Build and Publish](https://github.com/officialunofficial/waypoint/actions/workflows/docker-publish.yml/badge.svg)](https://github.com/officialunofficial/waypoint/actions/workflows/docker-publish.yml)
[![Docker Hub](https://img.shields.io/docker/pulls/officialunofficial/waypoint)](https://hub.docker.com/r/officialunofficial/waypoint)

![waypoint banner](banner.png)

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

## Getting Started

For detailed development instructions, see the [Development Guide](docs/development.md).

```bash
# Build the project
make build

# Run the main service
make run

# Queue backfill jobs
make backfill-queue                    # Queue all FIDs
make backfill-queue-fids FIDS=1,2,3    # Queue specific FIDs

# Run a backfill worker
make backfill-worker                   # Run backfill worker (50 concurrent jobs by default)
```

## Docker Development

```bash
# Use Docker Compose for local development with PostgreSQL 17 + pgvector
docker compose up

# Build Docker image
make docker-build

# Run the Docker container
make docker-run
```

## Environment Variables

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
BACKFILL_CONCURRENCY=50  # Number of concurrent FIDs to process

# Metrics configuration (optional)
WAYPOINT_STATSD__ENABLED=true
WAYPOINT_STATSD__ADDR=localhost:8125
WAYPOINT_STATSD__PREFIX=way_read
WAYPOINT_STATSD__USE_TAGS=false
```

You can also use a configuration file:

```bash
# Run with configuration file
WAYPOINT_CONFIG=config/sample.toml make run
```

## Metrics & Monitoring

Waypoint includes a StatsD-based metrics system with Grafana visualization:

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

For detailed metrics documentation, see [metrics.md](docs/metrics.md)

## Architecture

```mermaid
sequenceDiagram
    participant Hub as Snapchain
    participant Subscriber as HubSubscriber
    participant Redis as Redis Streams
    participant Consumer as StreamingService Consumer
    participant Processor as Event Processors
    participant DB as PostgreSQL Database

    rect rgb(191, 223, 255)
    note right of Hub: HubEvent Stream Processing
    
    Hub->>Subscriber: gRPC Event Stream (SubscribeRequest)
    activate Subscriber
    
    loop For Each Stream Event
        Hub->>Subscriber: HubEvent (MergeMessage, PruneMessage, etc.)
        Subscriber->>Subscriber: Filter spam events
        Subscriber->>Subscriber: Group by event type (casts, reactions, etc.)
        
        par Publish to Multiple Streams
            Subscriber->>Redis: Publish to "casts" stream
            Subscriber->>Redis: Publish to "reactions" stream
            Subscriber->>Redis: Publish to "links" stream
            Subscriber->>Redis: Publish to "user_data" stream
            Subscriber->>Redis: Publish to "verifications" stream
            Subscriber->>Redis: Publish to "username_proofs" stream
            Subscriber->>Redis: Publish to "onchain:*" streams
        end
        
        Subscriber->>Redis: Store last processed event ID
    end
    deactivate Subscriber
    end

    rect rgb(255, 204, 204)
    note right of Consumer: Consumer Group Processing
    
    par For Each Message Type
        Consumer->>Redis: Create consumer group
        
        loop Process Messages
            Consumer->>Redis: Reserve messages from stream (XREADGROUP)
            Redis->>Consumer: Batch of stream entries
            
            Consumer->>Processor: Process message batch
            
            par Process with Multiple Processors
                Processor->>Processor: DatabaseProcessor processes events
                Processor->>DB: Store events in database
                Processor->>Processor: PrintProcessor (debug logging)
            end
            
            Processor->>Consumer: Processing results
            Consumer->>Redis: Acknowledge messages (XACK)
            
            Consumer->>Redis: Claim stale messages (XCLAIM)
            Consumer->>Redis: Process claimed messages
        end
    end
    
    par Cleanup Tasks
        Consumer->>Redis: Trim old events (XTRIM)
    end
    end
```

For detailed architecture information, see [architecture.md](docs/architecture.md).

## Key Files

- `src/main.rs`: Main application entry point
- `src/backfill/reconciler.rs`: Message reconciliation logic
- `src/backfill/worker.rs`: Worker implementation

## Changelog

See [changelog.md](docs/changelog.md) for a detailed list of changes in each version.

## Contributing

Please see [contributing.md](docs/contributing.md) for details on contributing to this project.

