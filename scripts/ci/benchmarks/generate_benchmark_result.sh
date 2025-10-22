#!/bin/bash

# Generate benchmark results in benchmark-action format from wrk output
# Usage: ./generate_benchmark_result.sh <benchmark_results_directory>

set -e

RESULTS_DIR="${1:-artifacts}"

# Start JSON array
echo "["

FIRST=true

# Process all benchmark result files
for result in "$RESULTS_DIR"/benchmark_*.txt; do
    if [ -f "$result" ]; then
        ENDPOINT=$(basename "$result" | cut -d_ -f2 | cut -d. -f1)
        
        # Extract metrics from wrk output
        RPS=$(grep "Requests/sec:" "$result" | awk '{print $2}' || echo "0")
        LATENCY_AVG=$(grep "Latency" "$result" | head -1 | awk '{print $2}' | sed 's/[^0-9.]//g' || echo "0")
        
        # Extract percentiles - use tail to avoid matching "94.50%" from Req/Sec line
        LATENCY_P50=$(grep "50%" "$result" | tail -1 | awk '{print $2}' | sed 's/[^0-9.]//g' || echo "0")
        LATENCY_P90=$(grep "90%" "$result" | tail -1 | awk '{print $2}' | sed 's/[^0-9.]//g' || echo "0")
        LATENCY_P99=$(grep "99%" "$result" | tail -1 | awk '{print $2}' | sed 's/[^0-9.]//g' || echo "0")
        
        # Add comma separator if not first entry
        if [ "$FIRST" = true ]; then
            FIRST=false
        else
            echo ","
        fi
        
        # Output JSON entries for this endpoint
        cat << EOF
  {
    "name": "${ENDPOINT} - Avg Latency",
    "unit": "ms",
    "value": ${LATENCY_AVG}
  },
  {
    "name": "${ENDPOINT} - P50 Latency",
    "unit": "ms",
    "value": ${LATENCY_P50}
  },
  {
    "name": "${ENDPOINT} - P90 Latency",
    "unit": "ms",
    "value": ${LATENCY_P90}
  },
  {
    "name": "${ENDPOINT} - P99 Latency",
    "unit": "ms",
    "value": ${LATENCY_P99}
  },
  {
    "name": "${ENDPOINT} - Throughput",
    "unit": "req/sec",
    "value": ${RPS},
    "extra": "biggerIsBetter"
  }
EOF
    fi
done

# Close JSON array
echo "]"

