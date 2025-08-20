# Redis Migration Test Strategy

## Overview
Comprehensive testing approach to ensure the migration from bb8-redis to fred.rs preserves all existing functionality and improves performance.

## Test Categories

### 1. Unit Tests (`src/redis/tests.rs`)
Tests individual Redis operations in isolation.

#### Core Client Tests
- **Pool Initialization**: Verify pool creates with correct settings
- **Connection Management**: Test connection acquisition with timeouts
- **Pool Health Monitoring**: Verify pressure detection logic
- **Error Handling**: Test retry logic and error classification

#### Stream Operation Tests
- **XADD**: Message publishing with and without maxlen
- **XREADGROUP**: Consuming messages with blocking/non-blocking modes
- **XACK**: Message acknowledgment
- **XPENDING**: Retrieving pending messages
- **XCLAIM**: Claiming stale messages
- **XINFO**: Stream and group information queries
- **XLEN/XDEL/XTRIM**: Stream maintenance operations

#### List Operation Tests (for backfill)
- **LPUSH**: Adding items to queue
- **BRPOP**: Blocking pop from queue
- **LLEN**: Queue length check
- **LRANGE**: Retrieving queue items
- **LREM**: Removing specific items

### 2. Integration Tests (`tests/redis_integration.rs`)
Tests complete workflows with actual Redis instance.

#### Stream Processing Cycle
- Create consumer group
- Publish messages
- Consume messages
- Acknowledge processing
- Verify completion

#### Pending Message Recovery
- Simulate consumer failure
- Test stale message detection
- Verify message claiming
- Ensure no message loss

#### Concurrent Consumers
- Multiple consumers on same stream
- Verify message distribution
- Ensure exactly-once processing
- Test load balancing

#### Pool Stress Testing
- Simulate high concurrency (50+ workers)
- Measure success/timeout rates
- Verify pool recovery
- Test backpressure handling

### 3. Property-Based Tests
Verify invariants that must hold regardless of input.

#### Invariants to Test
- **Monotonic IDs**: Stream IDs always increase
- **No Message Loss**: All published messages are eventually consumed
- **Idempotent ACKs**: Multiple acknowledgments are safe
- **Order Preservation**: Messages consumed in order within partition

### 4. Performance Benchmarks
Establish baseline and compare implementations.

#### Metrics to Track
- **Throughput**: Messages per second
- **Latency**: P50, P95, P99 response times
- **Connection Efficiency**: Pool acquisition time
- **Memory Usage**: Heap allocation patterns
- **CPU Usage**: Under various loads

## Test Execution Plan

### Phase 1: Baseline (Current bb8-redis)
1. Run all tests against current implementation
2. Record performance metrics
3. Document any failing tests
4. Save results as baseline

### Phase 2: Migration Testing
1. Implement fred.rs client
2. Run unit tests - fix any failures
3. Run integration tests - verify behavior matches
4. Compare performance metrics

### Phase 3: Regression Testing
1. Run full test suite after each change
2. Ensure no functionality regression
3. Verify performance improvements

### Phase 4: Load Testing
1. Simulate production load (11+ concurrent streams)
2. Test with 1000+ messages/second
3. Verify no timeouts under normal load
4. Test recovery from pool exhaustion

## Critical Test Scenarios

### Scenario 1: Pool Exhaustion Recovery
```rust
// Exhaust pool with blocking operations
// Verify new requests handle gracefully
// Test automatic recovery
```

### Scenario 2: Network Interruption
```rust
// Simulate network failure
// Verify reconnection logic
// Ensure no message loss
```

### Scenario 3: Consumer Rebalancing
```rust
// Add/remove consumers dynamically
// Verify work redistribution
// Test pending message recovery
```

### Scenario 4: Backpressure Handling
```rust
// Overwhelm system with messages
// Verify graceful degradation
// Test flow control mechanisms
```

## Test Commands

```bash
# Run unit tests only
cargo test --lib

# Run integration tests (requires Redis)
cargo test --test redis_integration

# Run with coverage
cargo tarpaulin --out Html

# Run benchmarks
cargo bench --bench redis_performance

# Run property tests
cargo test --features proptest

# Run all tests including ignored
cargo test -- --ignored

# Run specific test pattern
cargo test redis_pool
```

## Success Criteria

### Functional
- ✅ All existing tests pass with fred
- ✅ No change in business logic behavior
- ✅ Error handling remains consistent
- ✅ All Redis commands work as expected

### Performance
- ✅ No pool timeout errors under normal load
- ✅ <1% timeout rate under stress
- ✅ Connection acquisition <10ms P99
- ✅ 20% improvement in throughput

### Reliability
- ✅ Automatic reconnection works
- ✅ No message loss during failures
- ✅ Graceful degradation under load
- ✅ Clean shutdown without data loss

## Rollback Criteria

If any of these occur, rollback to bb8-redis:
- Message loss in production
- >5% increase in error rate
- Performance degradation >10%
- Incompatible behavior changes
- Memory leaks detected

## Monitoring Post-Deployment

### Metrics to Watch
- Redis connection pool metrics
- Stream processing latency
- Error rates by type
- Message throughput
- Memory usage trends

### Alerts to Configure
- Pool exhaustion (>80% utilized)
- Timeout rate >1%
- Processing latency >1s P99
- Failed message rate >0.1%