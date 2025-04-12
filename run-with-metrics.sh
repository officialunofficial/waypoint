#!/bin/bash

# Configure StatsD metrics
export WAYPOINT_STATSD__PREFIX=way_read
export WAYPOINT_STATSD__ADDR=localhost:8125
export WAYPOINT_STATSD__USE_TAGS=false
export WAYPOINT_STATSD__ENABLED=true

# Run the command with metrics enabled
echo "Running with metrics enabled. Make sure the metrics stack is running with 'make metrics-start'"
"$@"