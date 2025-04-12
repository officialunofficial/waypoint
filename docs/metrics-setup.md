# Setting Up Metrics for Development

This guide explains how to set up and use the metrics infrastructure for Waypoint development.

## Quick Start

```bash
# Start the metrics infrastructure (Graphite/StatsD + Grafana)
make metrics-start

# Run your command with metrics enabled
./run-with-metrics.sh make backfill-worker

# View metrics in Grafana
make metrics-open  # Opens http://localhost:3000

# When done, stop the metrics infrastructure
make metrics-stop
```

## Metrics Stack Components

The metrics infrastructure consists of:

1. **Graphite/StatsD**: Collects and stores metrics data
   - Listens on UDP port 8125 for StatsD metrics
   - Provides a storage backend for time-series data

2. **Grafana**: Visualizes metrics in dashboards
   - Web interface available at http://localhost:3000
   - Preconfigured dashboards for Waypoint metrics
   - Username/password: admin/admin (first login)

## Running Locally with Metrics

To run any Waypoint command with metrics enabled:

```bash
./run-with-metrics.sh <your-command>
```

For example:
```bash
./run-with-metrics.sh make backfill-worker
./run-with-metrics.sh make run
./run-with-metrics.sh cargo run -- backfill fid queue
```

## Manual Configuration

If you need to manually configure metrics without using the helper script:

```bash
export WAYPOINT_STATSD__ENABLED=true
export WAYPOINT_STATSD__ADDR=localhost:8125
export WAYPOINT_STATSD__PREFIX=way_read
export WAYPOINT_STATSD__USE_TAGS=false

# Then run your command
make backfill-worker
```

## Docker Compose Configuration

The metrics infrastructure is defined in `grafana/docker-compose.yml` and includes:

- Graphite/StatsD container
- Grafana container
- Persistent storage for metrics data
- Preconfigured dashboards

## Troubleshooting

If metrics aren't showing up:

1. Check if the metrics services are running:
   ```bash
   cd grafana && docker compose ps
   ```

2. Verify that StatsD port is open:
   ```bash
   nc -uvz localhost 8125
   ```

3. Look for these log messages in Waypoint:
   ```
   "StatsD metrics initialized successfully"
   "StatsD connectivity test passed"
   ```

4. Try sending a test metric manually:
   ```bash
   echo "test.metric:1|c" | nc -u -w0 localhost 8125
   ```

5. Check Grafana configuration:
   - Ensure Graphite datasource is configured
   - Verify dashboard panels are using correct metric names