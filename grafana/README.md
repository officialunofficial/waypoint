# Waypoint Metrics with Grafana & StatsD

This directory contains the configuration for Grafana dashboards and datasources used to monitor Waypoint.

## Overview

Waypoint uses the following metrics stack:
- StatsD for metrics collection
- Graphite for metrics storage and aggregation
- Grafana for visualization and dashboards

## Available Dashboards

The default dashboard `waypoint-overview.json` provides key metrics:

1. **Backfill Metrics**
   - Jobs processed
   - FIDs processed
   - Processing rate (FIDs/second)

2. **Stream Metrics**
   - Events received
   - Event processing time

3. **System Metrics**
   - Memory usage
   - CPU usage

## Using the Metrics System in Waypoint Code

The metrics system is initialized in `src/main.rs` and provides helper functions in `src/metrics.rs`.

### Available Metric Types

1. **Counters** - Monotonically increasing values:
   ```rust
   // Example: Track number of jobs processed
   metrics::increment_jobs_processed();
   
   // Example: Track number of FIDs processed (with count)
   metrics::increment_fids_processed(count);
   ```

2. **Gauges** - Values that can go up and down:
   ```rust
   // Example: Track current number of jobs in queue
   metrics::set_jobs_in_queue(queue_length);
   
   // Example: Track processing rate
   metrics::set_backfill_fids_per_second(rate);
   ```

3. **Histograms** - Distribution of values:
   ```rust
   // Example: Record processing time
   metrics::record_event_processing_time(duration);
   ```

### Timing Operations

The `MetricsTimer` utility makes it easy to measure and record function execution times:

```rust
// Option 1: Use as a standalone timer
let timer = metrics::MetricsTimer::new("my_operation.timing");
// ... do work ...
drop(timer); // Metrics are recorded on drop

// Option 2: Time an async function
let result = metrics::time_async("my_operation.timing", async {
    // ... do async work ...
}).await;
```

## Adding New Metrics

To add new metrics:

1. Define new metric helper functions in `src/metrics.rs`
2. Call these functions from appropriate places in your code
3. Update the Grafana dashboard to display the new metrics

## Accessing Grafana

When running with docker-compose, Grafana is available at:
- URL: http://localhost:3000
- Default user/pass: admin/admin

## Configuration

StatsD configuration is set via environment variables:
- `METRICS_STATSD_HOST`: Host for StatsD server (default: "127.0.0.1")
- `METRICS_STATSD_PORT`: Port for StatsD server (default: 8125)
- `METRICS_PREFIX`: Prefix for all metrics (default: "snap_read")