# Hub Subscriber

The Hub Subscriber is a key component of Waypoint that establishes and maintains a connection to a Farcaster Hub, subscribes to events, and publishes them to Redis streams for processing.

## Overview

The Hub Subscriber provides real-time connectivity to a Farcaster Hub node, ensuring reliable data ingestion despite network instability, temporary disconnections, and server-side issues.

## Configuration

### Basic Configuration

The Hub Subscriber uses the Hub configuration specified in your config file:

```toml
[hub]
url = "snapchain.farcaster.xyz:3383"
retry_max_attempts = 5
retry_base_delay_ms = 100
retry_max_delay_ms = 30000
retry_jitter_factor = 0.25
retry_timeout_ms = 60000
conn_timeout_ms = 30000
```

These settings control connection retry behavior, which is crucial for maintaining a reliable connection to the Hub.

### Spam Filtering

The Hub Subscriber includes a built-in spam filter that can be enabled or disabled. By default, the spam filter is enabled to protect against spam messages.

When the spam filter is enabled, the subscriber loads a list of known spam accounts and filters out their messages before they reach the processing stage. This reduces database usage and improves performance by eliminating unwanted content.

To configure the spam filter setting:

```rust
// Create subscriber with spam filter explicitly enabled
let subscriber = HubSubscriber::new(
    client.clone(),
    redis.clone(),
    RedisStream::new(redis.clone()),
    "hub.hostname",
    "default".to_string(),
    SubscriberOptions {
        spam_filter_enabled: Some(true),
        ..Default::default()
    }
).await;

// Create subscriber with spam filter disabled
let subscriber = HubSubscriber::new(
    client.clone(),
    redis.clone(),
    RedisStream::new(redis.clone()),
    "hub.hostname",
    "default".to_string(),
    SubscriberOptions {
        spam_filter_enabled: Some(false),
        ..Default::default()
    }
).await;
```

Alternatively, this can be configured in the StreamingService initialization:

```rust
let streaming_service = StreamingService::new()
    .configure(|options| {
        // Configure other options
    })
    .with_spam_filter(true);  // Control spam filter here
```

## Advanced Options

The Hub Subscriber accepts several advanced options through the `SubscriberOptions` struct:

- `shard_index`: Specifies a shard to subscribe to for distributed processing
- `before_process`: Optional handler function called before processing events
- `after_process`: Optional handler function called after processing events
- `hub_config`: Custom hub configuration to override the default
- `spam_filter_enabled`: Controls whether the spam filter is active

## Resilience Features

The Hub Subscriber includes several resilience features:

1. **Exponential Backoff**: Retries connections with increasing delays
2. **Connection Monitoring**: Tracks successful operations and reconnects when necessary
3. **Error Classification**: Identifies different error types for appropriate handling
4. **H2 Protocol Error Handling**: Special handling for HTTP/2 protocol errors
5. **Automatic Reconnection**: Recreates the connection when problems are detected

## Architecture

The Hub Subscriber works in conjunction with the following components:

1. **Hub Client**: gRPC client for communicating with the Farcaster Hub
2. **Redis Client**: For storing processing state and last processed event ID
3. **Redis Stream**: For publishing events to be processed
4. **Spam Filter**: Optional component for filtering spam messages

## Processing Flow

1. The Hub Subscriber connects to the Farcaster Hub
2. It retrieves the last processed event ID from Redis
3. It subscribes to events from that ID forward
4. As events arrive, they are optionally filtered for spam
5. Valid events are published to Redis streams
6. The last processed event ID is updated in Redis

## Error Handling

The Hub Subscriber includes sophisticated error handling:

1. **Retryable Errors**: Connection issues and temporary failures
2. **Fatal Errors**: Fundamental configuration or permission problems
3. **Stream Management**: Handles unexpected stream closures and reconnections
4. **Backoff Strategy**: Uses exponential backoff with jitter to prevent thundering herd problems

## Monitoring

The Hub Subscriber tracks operational metrics including:

- Connection attempts
- Connection success rate
- Reconnection events
- Batch processing metrics
- Filtered message counts

Refer to the [metrics documentation](metrics.md) for more details.

## Customization

In most cases, the default settings are appropriate, but you can customize the subscriber behavior by adjusting retry parameters, batch sizes, and other options in the configuration.