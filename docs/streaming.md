# Streaming Service

The Streaming Service is a core component of Waypoint that handles real-time processing of Farcaster Hub events. This document describes how to configure and customize the service.

## Overview

The Streaming Service connects to the Farcaster Hub, subscribes to event streams, and processes these events in real-time. Events are first published to Redis streams, then consumed by processors which persist data to the database and perform other operations.

## Configuration

### Basic Configuration

The streaming service uses both the Hub and Redis configurations specified in your config file:

```toml
[hub]
url = "snapchain.farcaster.xyz:3383"
retry_max_attempts = 5
retry_base_delay_ms = 100
retry_max_delay_ms = 30000
retry_jitter_factor = 0.25
retry_timeout_ms = 60000
conn_timeout_ms = 30000

[redis]
url = "redis://localhost:6379"
pool_size = 5
batch_size = 100
enable_dead_letter = true
consumer_rebalance_interval_seconds = 300
metrics_collection_interval_seconds = 60
```

### Spam Filtering

New in recent versions, the streaming service includes a spam filter that can be enabled or disabled:

```rust
// Enable spam filter explicitly
let streaming_service = StreamingService::new()
    .with_spam_filter(true);

// Disable spam filter
let streaming_service = StreamingService::new()
    .with_spam_filter(false);
```

By default, the spam filter is enabled. When enabled, the system will filter out messages from known spam accounts, reducing database usage and improving processing performance.

### Print Processor

For debugging purposes, the streaming service includes a print processor that can log events to the console:

```rust
// Enable print processor for debugging
let streaming_service = StreamingService::new()
    .with_print_processor(true);

// Disable print processor (default for production)
let streaming_service = StreamingService::new()
    .with_print_processor(false);
```

The print processor is disabled by default to reduce log verbosity in production environments.

### Performance Tuning

The streaming service can be configured with various performance parameters:

```rust
let streaming_service = StreamingService::new()
    .configure(|options| {
        options
            .with_batch_size(10)           // Number of events to process in a batch
            .with_concurrency(200)         // Maximum concurrent processing tasks
            .with_timeout(Duration::from_secs(120))     // Processing timeout
            .with_retention(Duration::from_secs(24 * 60 * 60))  // Event retention in Redis
    });
```

These parameters can be tuned based on your hardware capabilities and processing requirements.

## Architecture

The streaming service consists of several components:

1. **HubSubscriber**: Connects to the Farcaster Hub and subscribes to events
2. **RedisStream**: Publishes events to Redis streams for reliable processing
3. **Consumer**: Reads events from Redis streams and processes them
4. **ProcessorRegistry**: Manages event processors like the database processor

## Event Processors

The streaming service supports multiple event processors:

1. **Database Processor**: Persists events to the PostgreSQL database (always enabled)
2. **Print Processor**: Outputs events to the console for debugging (optional)
3. **Spam Filter**: Filters out known spam accounts (optional but enabled by default)

## Processing Flow

1. The HubSubscriber connects to the Farcaster Hub
2. Events are received and published to Redis streams
3. The Consumer reads events from Redis in batches
4. Events are distributed to registered processors
5. Processors handle the events according to their logic
6. Successfully processed events are acknowledged in Redis

## Customizing with Code

You can customize the streaming service when initializing your application:

```rust
// In your application startup code
let streaming_service = StreamingService::new()
    .configure(|options| {
        options
            .with_batch_size(10)
            .with_concurrency(200)
    })
    .with_spam_filter(true)      // Enable spam filtering
    .with_print_processor(false); // Disable verbose logging

app.register_service(streaming_service);
```

This provides flexibility to configure the service based on your specific requirements.

## Monitoring

The streaming service emits metrics that can be tracked in Grafana:

- Event processing rate
- Processing latency
- Error rates
- Redis queue lengths
- Processing batch sizes

Refer to the [metrics documentation](metrics.md) for more details.