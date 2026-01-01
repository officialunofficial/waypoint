# Streaming

Real-time Snapchain event processing via gRPC → Redis → PostgreSQL.

## Configuration

```toml
[hub]
url = "snapchain.farcaster.xyz:3383"
retry_max_attempts = 5
conn_timeout_ms = 30000

[redis]
url = "redis://localhost:6379"
max_pool_size = 5
batch_size = 100
```

## Options

```rust
StreamingService::new()
    .configure(|opts| {
        opts.with_batch_size(10)
            .with_concurrency(200)
            .with_timeout(Duration::from_secs(120))
    })
    .with_spam_filter(true)      // default: on
    .with_print_processor(false) // default: off
```

## Components

1. **HubSubscriber** - connects to Snapchain gRPC
2. **RedisStream** - publishes to Redis for durability
3. **Consumer** - reads batches via XREADGROUP
4. **Processors** - DatabaseProcessor (persist), PrintProcessor (debug)

## Metrics

See [metrics.md](metrics.md) for available stream metrics.
