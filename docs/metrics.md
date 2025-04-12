# Waypoint Metrics System

Waypoint includes a comprehensive metrics system using StatsD, Graphite, and Grafana. This document explains how to use it.

## Quick Start

The metrics system runs in a separate Docker Compose stack located in the `grafana` directory.

### Running With Metrics

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

### Configuration

Metrics configuration is controlled via the `statsd` section in your config:

```toml
[statsd]
# Prefix for all metrics
prefix = "way_read"
# StatsD server address in host:port format
addr = "127.0.0.1:8125"
# Whether to use tagged metrics
use_tags = false
# Enable metrics collection
enabled = true
```

For local development, you can use the convenient wrapper script that automatically sets the correct environment variables:

```bash
# Run any command with metrics enabled
./run-with-metrics.sh make backfill-worker
```

## Architecture

The metrics system consists of:

1. **StatsD Client** - Built into Waypoint
2. **StatsD Server** - Runs in Docker, collects metrics
3. **Graphite** - Stores time-series data
4. **Grafana** - Visualizes metrics in dashboards

```
┌─────────────┐     UDP     ┌─────────────┐           ┌─────────────┐
│  Waypoint   ├────────────►│    StatsD   ├──────────►│  Graphite   │
│ Application │   (8125)    │   Server    │           │ (Storage)   │
└─────────────┘             └─────────────┘           └──────┬──────┘
                                                             │
                                                             ▼
                                                      ┌─────────────┐
                                                      │   Grafana   │
                                                      │ (Dashboard) │
                                                      └─────────────┘
```

## Available Metrics

### Backfill Metrics

- `backfill.jobs_processed` - Counter of jobs processed
- `backfill.fids_processed` - Counter of FIDs processed
- `backfill.jobs_in_queue` - Gauge of current jobs in queue
- `backfill.job_errors` - Counter of job errors
- `backfill.fids_per_second` - Gauge of processing rate

### Stream Metrics

- `stream.events_received` - Counter of events received
- `stream.events_processed` - Counter of events processed
- `stream.events_filtered` - Counter of events filtered out
- `stream.processing_time` - Histogram of event processing time

### System Metrics

- `system.memory_usage` - Gauge of memory usage in bytes
- `system.cpu_usage` - Gauge of CPU usage percentage

## Dashboard

The default Grafana dashboard is available at http://localhost:3050 and provides:

- Backfill progress tracking
- Stream processing metrics
- System resource usage

## Adding Custom Metrics

To add your own metrics:

1. Add helper functions in `src/metrics.rs`
2. Call these functions from your code
3. Update the Grafana dashboard to display them

Example of adding a custom counter:

```rust
// In metrics.rs
pub fn increment_my_custom_metric() {
    counter!("my_custom_metric", 1);
}

// In your code
use crate::metrics;
metrics::increment_my_custom_metric();
```

## Configuring Metrics

The metrics system follows the Waypoint configuration pattern with these settings:

### Environment Variables

```
WAYPOINT_STATSD__ENABLED=true                     # Enable/disable metrics (default: false)
WAYPOINT_STATSD__ADDR=localhost:8125              # StatsD server address in host:port format 
WAYPOINT_STATSD__PREFIX=way_read                 # Prefix for all metrics (default: "way_read")
WAYPOINT_STATSD__USE_TAGS=false                   # Whether to use tagged metrics (default: false)
```

### TOML Configuration

You can also configure metrics in your config file:

```toml
[statsd]
enabled = true
addr = "localhost:8125"
prefix = "way_read"
use_tags = false
```

## Troubleshooting

If metrics aren't showing up:

1. Verify the metrics infrastructure is running:
   ```bash
   cd grafana && docker compose ps
   ```

2. Check if StatsD port is open:
   ```bash
   nc -uvz localhost 8125
   ```

3. Verify Waypoint is sending metrics:
   ```bash
   # Look for these log messages
   "StatsD metrics initialized successfully with endpoint localhost:8125"
   "StatsD connectivity test passed"
   ```

4. Try sending a test metric manually:
   ```bash
   echo "test.metric:1|c" | nc -u -w0 localhost 8125
   ```

5. Check Grafana is configured with the Graphite datasource:
   - Go to http://localhost:3000/datasources
   - Verify the Graphite datasource is configured