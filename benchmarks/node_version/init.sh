#!/bin/bash

# Node version endpoint benchmark
# This script assumes the server is already running on localhost:8080
# Usage: ./init.sh [scenario] [hardware_profile]

set -e

# Get configuration from benchmark_config.json
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CONFIG_FILE="$PROJECT_ROOT/benchmark_config.json"

# Parse arguments
SCENARIO="${1:-light_load}"
HARDWARE_PROFILE="${2:-ci_runner}"

# Validate hardware profile and get default scenario if needed
if ! jq -e ".hardware_profiles.\"$HARDWARE_PROFILE\"" "$CONFIG_FILE" > /dev/null; then
    echo "Error: Hardware profile '$HARDWARE_PROFILE' not found in config"
    echo "Available profiles:"
    jq -r '.hardware_profiles | keys[]' "$CONFIG_FILE"
    exit 1
fi

# Check if scenario is supported by hardware profile
SUPPORTED_SCENARIOS=$(jq -r ".hardware_profiles.\"$HARDWARE_PROFILE\".scenarios[]" "$CONFIG_FILE")
if ! echo "$SUPPORTED_SCENARIOS" | grep -q "^$SCENARIO$"; then
    echo "Warning: Scenario '$SCENARIO' not recommended for hardware profile '$HARDWARE_PROFILE'"
    echo "Recommended scenarios: $SUPPORTED_SCENARIOS"
    echo "Continuing anyway..."
fi

# Get benchmark configuration
# Try custom scenarios first, fall back to standard scenarios
THREADS=$(jq -r ".benchmarks.node_version.custom_scenarios[]? | select(.name == \"$SCENARIO\") | .threads" "$CONFIG_FILE")
if [ -z "$THREADS" ] || [ "$THREADS" == "null" ]; then
    THREADS=$(jq -r ".standard_scenarios[] | select(.name == \"$SCENARIO\") | .threads" "$CONFIG_FILE")
fi

CONNECTIONS=$(jq -r ".benchmarks.node_version.custom_scenarios[]? | select(.name == \"$SCENARIO\") | .connections" "$CONFIG_FILE")
if [ -z "$CONNECTIONS" ] || [ "$CONNECTIONS" == "null" ]; then
    CONNECTIONS=$(jq -r ".standard_scenarios[] | select(.name == \"$SCENARIO\") | .connections" "$CONFIG_FILE")
fi

DURATION=$(jq -r ".benchmarks.node_version.custom_scenarios[]? | select(.name == \"$SCENARIO\") | .duration" "$CONFIG_FILE")
if [ -z "$DURATION" ] || [ "$DURATION" == "null" ]; then
    DURATION=$(jq -r ".standard_scenarios[] | select(.name == \"$SCENARIO\") | .duration" "$CONFIG_FILE")
fi

TIMEOUT=$(jq -r ".benchmarks.node_version.custom_scenarios[]? | select(.name == \"$SCENARIO\") | .timeout" "$CONFIG_FILE")
if [ -z "$TIMEOUT" ] || [ "$TIMEOUT" == "null" ]; then
    TIMEOUT=$(jq -r ".standard_scenarios[] | select(.name == \"$SCENARIO\") | .timeout" "$CONFIG_FILE")
fi

if [ -z "$THREADS" ] || [ "$THREADS" == "null" ]; then
    echo "Error: Scenario '$SCENARIO' not found in config"
    exit 1
fi

# Get server configuration
SERVER_HOST=$(jq -r '.server.host' "$CONFIG_FILE")
SERVER_PORT=$(jq -r '.server.port' "$CONFIG_FILE")

# Get hardware profile description
PROFILE_DESC=$(jq -r ".hardware_profiles.\"$HARDWARE_PROFILE\".description" "$CONFIG_FILE")

echo "Running node_version endpoint benchmark: $SCENARIO"
echo "Hardware profile: $HARDWARE_PROFILE ($PROFILE_DESC)"
echo "Configuration: threads=$THREADS, connections=$CONNECTIONS, duration=$DURATION, timeout=${TIMEOUT:-120s}"

# Run wrk benchmark
cd "$SCRIPT_DIR"
wrk -d"$DURATION" -t"$THREADS" -c"$CONNECTIONS" --timeout "${TIMEOUT:-120s}" --latency \
    -s ./node_version.lua "http://$SERVER_HOST:$SERVER_PORT"
