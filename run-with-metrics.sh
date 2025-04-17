#!/bin/bash

# Script to run Waypoint with metrics enabled
# Usage: ./run-with-metrics.sh [command]
# Example: ./run-with-metrics.sh cargo run -- start

# Import environment variables from .env file if it exists
if [ -f .env ]; then
  export $(grep -v '^#' .env | xargs)
fi

# Configure StatsD metrics
export WAYPOINT_STATSD__PREFIX=${WAYPOINT_STATSD__PREFIX:-way_read}
export WAYPOINT_STATSD__ADDR=${WAYPOINT_STATSD__ADDR:-localhost:8125}
export WAYPOINT_STATSD__USE_TAGS=${WAYPOINT_STATSD__USE_TAGS:-false}
export WAYPOINT_STATSD__ENABLED=true

# Run the command with metrics enabled
echo "Running with metrics enabled. Make sure the metrics stack is running with 'make metrics-start'"
"$@"