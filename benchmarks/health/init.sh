#!/bin/bash

# Health endpoint benchmark
# This script assumes the server is already running on localhost:8080

set -e

# Get configuration from benchmark_config.json
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CONFIG_FILE="$PROJECT_ROOT/benchmark_config.json"

# Parse scenario from argument or use default
SCENARIO="${1:-light_load}"

# Get benchmark configuration
THREADS=$(jq -r ".benchmarks.health.scenarios[] | select(.name == \"$SCENARIO\") | .threads" "$CONFIG_FILE")
CONNECTIONS=$(jq -r ".benchmarks.health.scenarios[] | select(.name == \"$SCENARIO\") | .connections" "$CONFIG_FILE")
DURATION=$(jq -r ".benchmarks.health.scenarios[] | select(.name == \"$SCENARIO\") | .duration" "$CONFIG_FILE")

if [ -z "$THREADS" ] || [ "$THREADS" == "null" ]; then
    echo "Error: Scenario '$SCENARIO' not found in config"
    exit 1
fi

# Get server configuration
SERVER_HOST=$(jq -r '.server.host' "$CONFIG_FILE")
SERVER_PORT=$(jq -r '.server.port' "$CONFIG_FILE")

echo "Running health endpoint benchmark: $SCENARIO"
echo "Configuration: threads=$THREADS, connections=$CONNECTIONS, duration=$DURATION"

# Run wrk benchmark
cd "$SCRIPT_DIR"
wrk -d"$DURATION" -t"$THREADS" -c"$CONNECTIONS" --timeout 120s --latency \
    -s ./health.lua "http://$SERVER_HOST:$SERVER_PORT"
