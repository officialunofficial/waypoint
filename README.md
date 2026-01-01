# Waypoint

Waypoint is a Snapchain synchronization tool built in Rust, optimized for memory efficiency. It provides a streaming synchronization service combined with backfill capabilities to process historical data.

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

### MCP Service
- **Model Context Protocol**: Provides AI assistants with access to Farcaster data
- **Available Tools**: Fetch user profiles (by FID or username), verifications, casts, reactions, and links
- **Username Search**: Find users by their Farcaster username instead of requiring FID
- **Social Graph**: Explore follow relationships with default "follow" link type
- **Documentation**: See [mcp.md](docs/mcp.md) for details

### Ethereum Module
- **Wallet Management**: Create and manage Ethereum wallets using mnemonic phrases
- **Provider Integration**: Connect to Ethereum networks via Alchemy API
- **Network Support**: Built-in support for Base Chain, Ethereum mainnet, and testnets
- **Secure Key Handling**: Zeroize-based secure mnemonic storage
- **Examples**: Sample code for wallet operations and network interactions

## Getting Started

For detailed development instructions, see the [Development Guide](docs/development.md).

```bash
# Create and configure your environment
make env-setup
# Edit the .env file to configure your settings

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

# Start the main service
docker compose up -d

# Start the backfill services (uses the same DB and Redis)
docker compose --profile backfill up -d

# Scale backfill workers for faster processing
docker compose --profile backfill up -d --scale backfill-worker=4
# Or use environment variable:
BACKFILL_WORKERS=4 docker compose --profile backfill up -d

# Start with local Snapchain (optional, requires configuration)
docker compose --profile snapchain up -d

# Build Docker image
make docker-build

# Run the Docker container
make docker-run
```

### Backfill Configuration

You can configure the backfill behavior using these environment variables:

| Variable | Description | Default |
|----------|-------------|---------|
| `BACKFILL_BATCH_SIZE` | Number of FIDs per batch when queueing | 50 |
| `BACKFILL_CONCURRENCY` | Concurrent FIDs per worker | 40 |
| `BACKFILL_WORKERS` | Number of worker container replicas | 1 |

### Backfill Architecture

The backfill system uses a queue/worker architecture:

1. **Queue Service** (`backfill-queue`): Populates the Redis queue with FID batches, then exits
2. **Worker Service** (`backfill-worker`): Processes jobs from the queue until complete

Workers automatically exit when the queue has been empty for 60 seconds (configurable via `--idle-timeout`). Multiple workers can process the same queue concurrently for faster backfills.

## Configuration

Waypoint can be configured using environment variables in a `.env` file. Use `make env-setup` to create a default `.env` file from the example.

The main configuration options are:

```
# Database configuration
POSTGRES_USER=postgres
POSTGRES_PASSWORD=postgres
POSTGRES_DB=waypoint
DATABASE_URL=postgresql://postgres:postgres@localhost:5432/waypoint
WAYPOINT_DATABASE__URL=${DATABASE_URL}

# Redis configuration
WAYPOINT_REDIS__URL=redis://localhost:6379

# Farcaster Hub configuration
WAYPOINT_HUB__URL=snapchain.farcaster.xyz:3383  # HTTPS is used by default
# For local development with HTTP:
# WAYPOINT_HUB__URL=http://localhost:2283
# Explicit HTTPS:
# WAYPOINT_HUB__URL=https://snapchain.farcaster.xyz:3383

# Optional: Custom headers for authenticated hubs
# Note: Environment variable names use uppercase with underscores, but the actual header names
# sent will be lowercase with hyphens (e.g., X_API_KEY becomes x-api-key header)
# WAYPOINT_HUB__HEADERS__X_API_KEY=your_api_key_here
# WAYPOINT_HUB__HEADERS__AUTHORIZATION=Bearer your_token_here

# MCP service configuration
WAYPOINT_MCP__ENABLED=true
WAYPOINT_MCP__BIND_ADDRESS=127.0.0.1  # Use 0.0.0.0 to allow external connections
WAYPOINT_MCP__PORT=8000

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

# Ethereum configuration (optional)
WAYPOINT_ETH__ALCHEMY_API_KEY=your_alchemy_api_key
WAYPOINT_ETH__DEFAULT_NETWORK=base  # Options: mainnet, base, sepolia, base-sepolia, goerli
WAYPOINT_ETH__MNEMONIC="your mnemonic phrase here"
# WAYPOINT_ETH__DERIVATION_PATH=m/44'/60'/0'/0/0
```

See `.env.example` for a complete list of configuration options.

### Configuration Prefixes

Waypoint uses Figment for configuration, which requires specific prefixes:
- `WAYPOINT_` prefix is used for application configuration (with double underscore for nesting)
- `POSTGRES_USER`, `POSTGRES_PASSWORD`, and `POSTGRES_DB` are used by Docker Compose for PostgreSQL initialization
- `DATABASE_URL` is used by SQLx for database schema management

### Docker Configuration

When using Docker or Docker Compose, the following connection strings are modified to use container hostnames:

```
# For Docker Compose
WAYPOINT_DATABASE__URL=postgresql://postgres:postgres@postgres:5432/waypoint
WAYPOINT_REDIS__URL=redis://redis:6379

# For standalone Docker (using host.docker.internal)
WAYPOINT_DATABASE__URL=postgresql://postgres:postgres@host.docker.internal:5432/waypoint
WAYPOINT_REDIS__URL=redis://host.docker.internal:6379
```

These settings are automatically set in the docker-compose.yml and Makefile.

### Connecting to Local Snapchain in Docker

**Important:** When running Waypoint in Docker, **never use `localhost`** for the hub URL. In Docker containers, `localhost` refers to the container itself, not to other containers or the host machine.

If you're running both Waypoint and Snapchain in Docker containers:

**Docker Compose (containers on the same network):**
```bash
WAYPOINT_HUB__URL=http://snapchain:3381
```
Replace `snapchain` with your Snapchain service name from your docker-compose.yml file.

**Standalone Docker (Docker Desktop on macOS/Windows):**
```bash
WAYPOINT_HUB__URL=http://host.docker.internal:3381
```

**Standalone Docker (Linux hosts):**
Use your host's IP address on the Docker bridge network, or use host network mode:
```bash
# Option 1: Use host IP
WAYPOINT_HUB__URL=http://172.17.0.1:3381

# Option 2: Run with host network mode
docker run --network host ...
```

**Running with local Snapchain using docker-compose:**
```bash
# Set the hub URL in your .env file
WAYPOINT_HUB__URL=http://snapchain:3381

# Start Waypoint with local Snapchain
docker compose --profile snapchain up
```

The Snapchain service is included in docker-compose.yml as an optional profile. Note: You'll need to adjust the Snapchain image and configuration for your specific setup.

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
%%{init: {'theme': 'base', 'themeVariables': { 'background': '#f5f5f5' }}}%%
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
            
            par Process with Processors
                Processor->>Processor: DatabaseProcessor processes events
                Processor->>DB: Store events in database
                Note over Processor: PrintProcessor disabled by default
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

