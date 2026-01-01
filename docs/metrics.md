# Metrics

StatsD → Graphite → Grafana pipeline in the `grafana/` directory.

## Usage

```bash
make metrics-start
./run-with-metrics.sh make backfill-worker
make metrics-open    # http://localhost:3050
make metrics-stop
```

## Configuration

```bash
WAYPOINT_STATSD__ENABLED=true
WAYPOINT_STATSD__ADDR=localhost:8125
WAYPOINT_STATSD__PREFIX=way_read
```

## Available Metrics

**Backfill:**
- `backfill.jobs_processed`
- `backfill.fids_processed`
- `backfill.jobs_in_queue`
- `backfill.job_errors`
- `backfill.fids_per_second`

**Streaming:**
- `stream.events_received`
- `stream.events_processed`
- `stream.events_filtered`
- `stream.processing_time`

## Adding Metrics

```rust
// src/metrics.rs
pub fn increment_my_metric() {
    counter!("my_metric", 1);
}

// usage
metrics::increment_my_metric();
```

## Troubleshooting

```bash
cd grafana && docker compose ps     # check services
nc -uvz localhost 8125              # test StatsD port
echo "test:1|c" | nc -u -w0 localhost 8125  # send test metric
```
