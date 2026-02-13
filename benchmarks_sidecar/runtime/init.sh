#!/bin/bash
# Copyright (C) 2026 Parity Technologies (UK) Ltd.
# SPDX-License-Identifier: GPL-3.0-or-later


# Sidecar Runtime/spec endpoint benchmark
# Usage: ./init.sh [scenario] [hardware_profile]

set -e

# Get configuration
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CONFIG_FILE="$PROJECT_ROOT/sidecar_mapping.json"

# Parse arguments
SCENARIO="${1:-light_load}"
HARDWARE_PROFILE="${2:-ci_runner}"

# Get benchmark configuration
THREADS=$(jq -r ".benchmarks.runtime.scenarios[] | select(.name == \"$SCENARIO\") | .threads" "$CONFIG_FILE")
CONNECTIONS=$(jq -r ".benchmarks.runtime.scenarios[] | select(.name == \"$SCENARIO\") | .connections" "$CONFIG_FILE")
DURATION=$(jq -r ".benchmarks.runtime.scenarios[] | select(.name == \"$SCENARIO\") | .duration" "$CONFIG_FILE")
TIMEOUT=$(jq -r ".benchmarks.runtime.scenarios[] | select(.name == \"$SCENARIO\") | .timeout" "$CONFIG_FILE")

if [ -z "$THREADS" ] || [ "$THREADS" == "null" ]; then
    echo "Error: Scenario '$SCENARIO' not found in config"
    exit 1
fi

# Get server configuration
SERVER_HOST=$(jq -r '.sidecar_server.host' "$CONFIG_FILE")
SERVER_PORT=$(jq -r '.sidecar_server.port' "$CONFIG_FILE")
PROTOCOL=$(jq -r '.sidecar_server.protocol' "$CONFIG_FILE")

echo "Running Sidecar runtime/spec endpoint benchmark: $SCENARIO"
echo "Configuration: threads=$THREADS, connections=$CONNECTIONS, duration=$DURATION, timeout=${TIMEOUT:-120s}"

# Run wrk benchmark
cd "$SCRIPT_DIR"
wrk -d"$DURATION" -t"$THREADS" -c"$CONNECTIONS" --timeout "${TIMEOUT:-120s}" --latency \
    -s ./runtime.lua "${PROTOCOL}://$SERVER_HOST:$SERVER_PORT"

