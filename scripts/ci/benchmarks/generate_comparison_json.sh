#!/bin/bash
# Copyright (C) 2026 Parity Technologies (UK) Ltd.
# SPDX-License-Identifier: GPL-3.0-or-later


set -e

LOCAL_DIR="${1:-artifacts}"
SIDECAR_DIR="${2:-artifacts_sidecar}"

# Function to extract metric from wrk output
extract_metric() {
    local file=$1
    local metric=$2
    
    case $metric in
        "rps")
            grep "Requests/sec:" "$file" | awk '{print $2}' || echo "0"
            ;;
        "latency_avg")
            grep "Latency" "$file" | head -1 | awk '{print $2}' | sed 's/[^0-9.]//g' || echo "0"
            ;;
        "latency_p99")
            grep "99%" "$file" | tail -1 | awk '{print $2}' | sed 's/[^0-9.]//g' || echo "0"
            ;;
    esac
}

# Function to calculate percentage difference
calc_diff() {
    local local_val=$1
    local sidecar_val=$2
    
    if [ "$sidecar_val" == "0" ] || [ -z "$sidecar_val" ]; then
        echo "0"
        return
    fi
    
    # For throughput: positive = better (local is faster)
    # For latency: positive = better (local is faster, i.e. lower latency)
    echo "scale=2; (($local_val - $sidecar_val) / $sidecar_val) * 100" | bc
}

# Start JSON array
echo "["

FIRST=true

# Get list of endpoints from local results
for local_result in "$LOCAL_DIR"/benchmark_*.txt; do
    if [ -f "$local_result" ]; then
        ENDPOINT=$(basename "$local_result" .txt)
        ENDPOINT=${ENDPOINT#benchmark_}
        SIDECAR_RESULT="$SIDECAR_DIR/benchmark_sidecar_${ENDPOINT}.txt"
        
        if [ ! -f "$SIDECAR_RESULT" ]; then
            echo "Warning: No sidecar result for $ENDPOINT" >&2
            continue
        fi
        
        # Extract metrics
        local_rps=$(extract_metric "$local_result" "rps")
        sidecar_rps=$(extract_metric "$SIDECAR_RESULT" "rps")
        
        local_lat_avg=$(extract_metric "$local_result" "latency_avg")
        sidecar_lat_avg=$(extract_metric "$SIDECAR_RESULT" "latency_avg")
        
        local_lat_p99=$(extract_metric "$local_result" "latency_p99")
        sidecar_lat_p99=$(extract_metric "$SIDECAR_RESULT" "latency_p99")
        
        # Calculate improvements (positive = local is better)
        throughput_improvement=$(calc_diff "$local_rps" "$sidecar_rps")
        
        latency_improvement=$(echo "scale=2; (($sidecar_lat_avg - $local_lat_avg) / $sidecar_lat_avg) * 100" | bc)
        latency_p99_improvement=$(echo "scale=2; (($sidecar_lat_p99 - $local_lat_p99) / $sidecar_lat_p99) * 100" | bc)
        
        # Add comma separator if not first entry
        if [ "$FIRST" = true ]; then
            FIRST=false
        else
            echo ","
        fi
        
        cat << EOF
  {
    "name": "${ENDPOINT} - Throughput Improvement vs Sidecar",
    "unit": "%",
    "value": ${throughput_improvement},
    "extra": "biggerIsBetter"
  },
  {
    "name": "${ENDPOINT} - Avg Latency Improvement vs Sidecar",
    "unit": "%",
    "value": ${latency_improvement},
    "extra": "biggerIsBetter"
  },
  {
    "name": "${ENDPOINT} - P99 Latency Improvement vs Sidecar",
    "unit": "%",
    "value": ${latency_p99_improvement},
    "extra": "biggerIsBetter"
  },
  {
    "name": "${ENDPOINT} - Local Throughput",
    "unit": "req/sec",
    "value": ${local_rps},
    "extra": "biggerIsBetter"
  },
  {
    "name": "${ENDPOINT} - Sidecar Throughput",
    "unit": "req/sec",
    "value": ${sidecar_rps},
    "extra": "biggerIsBetter"
  },
  {
    "name": "${ENDPOINT} - Local Avg Latency",
    "unit": "ms",
    "value": ${local_lat_avg}
  },
  {
    "name": "${ENDPOINT} - Sidecar Avg Latency",
    "unit": "ms",
    "value": ${sidecar_lat_avg}
  }
EOF
    fi
done

# Close JSON array
echo ""
echo "]"

