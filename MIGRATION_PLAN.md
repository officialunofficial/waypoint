# Migration Plan: bb8-redis to fred.rs

## Overview
Replace bb8-redis + bb8 connection pooling with fred.rs, which provides built-in connection pooling, automatic reconnection, and better async support.

## Current Issues with bb8-redis
- Connection pool exhaustion (18 connections, 0 idle)
- Frequent timeouts under load
- Manual connection management overhead
- Complex error handling for pool operations

## Benefits of fred.rs
- Built-in connection pooling with automatic management
- Automatic reconnection and retry logic
- Native async/await support
- Better performance with pipelining
- Simpler API
- Active maintenance and modern design

## Migration Steps

### Phase 1: Setup and Dependencies
1. Update Cargo.toml:
   - Remove: `bb8 = "0.9.0"` and `bb8-redis = "0.24.0"`
   - Add: `fred = { version = "9.1", features = ["enable-native-tls", "partial-tracing", "custom-reconnect-errors"] }`

### Phase 2: Core Redis Client (`src/redis/client.rs`)
1. Replace `bb8::Pool<RedisConnectionManager>` with `fred::clients::RedisPool`
2. Update connection initialization:
   - Replace bb8 pool builder with fred's `RedisPool::new`
   - Configure fred's connection options (max_connections, auto_pipeline, etc.)
3. Remove manual pool health monitoring (fred handles this internally)
4. Update connection methods to use fred's API

### Phase 3: Stream Operations (`src/redis/stream.rs`)
1. Replace stream commands:
   - `bb8_redis::redis::cmd("XADD")` → `client.xadd()`
   - `bb8_redis::redis::cmd("XREADGROUP")` → `client.xreadgroup()`
   - `bb8_redis::redis::cmd("XACK")` → `client.xack()`
   - `bb8_redis::redis::cmd("XPENDING")` → `client.xpending()`
   - `bb8_redis::redis::cmd("XCLAIM")` → `client.xclaim()`
   - etc.

### Phase 4: Error Handling (`src/redis/error.rs`)
1. Replace `bb8_redis::redis::RedisError` with `fred::error::RedisError`
2. Update error matching patterns
3. Simplify retry logic (fred handles most retries internally)

### Phase 5: Service Updates
1. Update `src/services/streaming.rs`:
   - Remove manual connection retrieval
   - Use fred's direct command methods
   - Simplify error handling
2. Update `src/processor/stream.rs`:
   - Similar changes as streaming.rs
3. Update `src/backfill/worker.rs`:
   - Update list operations to use fred

### Phase 6: Configuration
1. Update `src/config.rs`:
   - Adjust pool configuration for fred
   - Remove connection_timeout_ms (fred handles internally)
   - Add fred-specific options if needed

## Command Mapping

| bb8-redis | fred |
|-----------|------|
| `bb8_redis::redis::cmd("XADD")` | `client.xadd()` |
| `bb8_redis::redis::cmd("XREADGROUP")` | `client.xreadgroup()` |
| `bb8_redis::redis::cmd("XACK")` | `client.xack()` |
| `bb8_redis::redis::cmd("XPENDING")` | `client.xpending()` |
| `bb8_redis::redis::cmd("XCLAIM")` | `client.xclaim()` |
| `bb8_redis::redis::cmd("XINFO")` | `client.xinfo_stream()` / `client.xinfo_groups()` |
| `bb8_redis::redis::cmd("XLEN")` | `client.xlen()` |
| `bb8_redis::redis::cmd("XDEL")` | `client.xdel()` |
| `bb8_redis::redis::cmd("XTRIM")` | `client.xtrim()` |
| `bb8_redis::redis::cmd("GET")` | `client.get()` |
| `bb8_redis::redis::cmd("SET")` | `client.set()` |
| `bb8_redis::redis::cmd("PING")` | `client.ping()` |
| `bb8_redis::redis::cmd("LPUSH")` | `client.lpush()` |
| `bb8_redis::redis::cmd("BRPOP")` | `client.brpop()` |

## Value Type Mapping

| bb8-redis | fred |
|-----------|------|
| `bb8_redis::redis::Value::Array` | `fred::types::RedisValue::Array` |
| `bb8_redis::redis::Value::BulkString` | `fred::types::RedisValue::Bytes` |
| `bb8_redis::redis::Value::Int` | `fred::types::RedisValue::Integer` |
| `bb8_redis::redis::Value::Nil` | `fred::types::RedisValue::Nil` |

## Testing Strategy
1. Unit tests for each Redis operation
2. Integration tests for stream processing
3. Load testing to verify connection pool improvements
4. Gradual rollout with monitoring

## Rollback Plan
- Keep the branch separate until fully tested
- Tag the last bb8-redis version
- Document any breaking changes
- Have metrics to compare performance

## Files to Update (Priority Order)
1. `Cargo.toml` - Dependencies
2. `src/redis/client.rs` - Core client implementation
3. `src/redis/error.rs` - Error types
4. `src/redis/stream.rs` - Stream operations
5. `src/redis/types.rs` - Type definitions (if needed)
6. `src/config.rs` - Configuration
7. `src/services/streaming.rs` - Stream service
8. `src/processor/stream.rs` - Stream processor
9. `src/backfill/worker.rs` - Backfill operations
10. `src/commands/backfill/fid/worker.rs` - FID worker