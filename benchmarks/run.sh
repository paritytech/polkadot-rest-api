#!/bin/bash
# Copyright (C) 2026 Parity Technologies (UK) Ltd.
# SPDX-License-Identifier: GPL-3.0-or-later


# Unified benchmark runner script
# 
# Usage: ./run.sh <benchmark_name> [scenario] [hardware_profile]
#
# Examples:
#   ./run.sh health
#   ./run.sh blocks light_load ci_runner
#   ./run.sh ahm_info medium_load local_dev

set -e

BENCHMARKS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$BENCHMARKS_DIR/.." && pwd)"
CONFIG_FILE="$PROJECT_ROOT/benchmark_config.json"

# Parse arguments
BENCHMARK_NAME="${1:-}"
SCENARIO="${2:-light_load}"
HARDWARE_PROFILE="${3:-ci_runner}"

if [ -z "$BENCHMARK_NAME" ]; then
    echo "Usage: $0 <benchmark_name> [scenario] [hardware_profile]"
    echo ""
    echo "Available benchmarks:"
    for dir in "$BENCHMARKS_DIR"/*/; do
        name=$(basename "$dir")
        if [ -f "$dir/${name}.lua" ]; then
            echo "  - $name"
        fi
    done
    exit 1
fi

# Validate benchmark exists
SCRIPT_DIR="$BENCHMARKS_DIR/$BENCHMARK_NAME"
if [ ! -d "$SCRIPT_DIR" ] || [ ! -f "$SCRIPT_DIR/${BENCHMARK_NAME}.lua" ]; then
    echo "Error: Benchmark '$BENCHMARK_NAME' not found"
    echo "Expected directory: $SCRIPT_DIR"
    echo "Expected lua file: $SCRIPT_DIR/${BENCHMARK_NAME}.lua"
    exit 1
fi

# Validate hardware profile
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

# Helper function to get config value with fallback
get_config_value() {
    local key="$1"
    local value
    
    # Try custom scenarios first
    value=$(jq -r ".benchmarks.\"$BENCHMARK_NAME\".custom_scenarios[]? | select(.name == \"$SCENARIO\") | .$key" "$CONFIG_FILE")
    
    # Fall back to standard scenarios
    if [ -z "$value" ] || [ "$value" == "null" ]; then
        value=$(jq -r ".standard_scenarios[] | select(.name == \"$SCENARIO\") | .$key" "$CONFIG_FILE")
    fi
    
    echo "$value"
}

# Get benchmark configuration
THREADS=$(get_config_value "threads")
CONNECTIONS=$(get_config_value "connections")
DURATION=$(get_config_value "duration")
TIMEOUT=$(get_config_value "timeout")

if [ -z "$THREADS" ] || [ "$THREADS" == "null" ]; then
    echo "Error: Scenario '$SCENARIO' not found in config"
    exit 1
fi

# Get server configuration
SERVER_HOST=$(jq -r '.server.host' "$CONFIG_FILE")
SERVER_PORT=$(jq -r '.server.port' "$CONFIG_FILE")

# Get hardware profile description
PROFILE_DESC=$(jq -r ".hardware_profiles.\"$HARDWARE_PROFILE\".description" "$CONFIG_FILE")

# Generate display name from benchmark name (replace underscores with spaces)
DISPLAY_NAME=$(echo "$BENCHMARK_NAME" | tr '_' ' ')

echo "Running $DISPLAY_NAME endpoint benchmark: $SCENARIO"
echo "Hardware profile: $HARDWARE_PROFILE ($PROFILE_DESC)"
echo "Configuration: threads=$THREADS, connections=$CONNECTIONS, duration=$DURATION, timeout=${TIMEOUT:-120s}"

# Run wrk benchmark
cd "$SCRIPT_DIR"
# Set LUA_PATH to include the benchmarks directory for require("../util")
export LUA_PATH="$BENCHMARKS_DIR/?.lua;;"
wrk -d"$DURATION" -t"$THREADS" -c"$CONNECTIONS" --timeout "${TIMEOUT:-120s}" --latency \
    -s "./${BENCHMARK_NAME}.lua" "http://$SERVER_HOST:$SERVER_PORT"
