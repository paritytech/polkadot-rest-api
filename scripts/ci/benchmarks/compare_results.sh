#!/bin/bash

set -e

LOCAL_DIR="${1:-artifacts}"
SIDECAR_DIR="${2:-artifacts_sidecar}"
MAPPING_FILE="sidecar_mapping.json"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo "=================================================="
echo "  Benchmark Comparison: Local API vs Sidecar"
echo "=================================================="
echo ""

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
        "latency_p50")
            grep "50%" "$file" | tail -1 | awk '{print $2}' | sed 's/[^0-9.]//g' || echo "0"
            ;;
        "latency_p90")
            grep "90%" "$file" | tail -1 | awk '{print $2}' | sed 's/[^0-9.]//g' || echo "0"
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
        echo "N/A"
        return
    fi
    
    # Calculate percentage difference: ((local - sidecar) / sidecar) * 100
    echo "scale=2; (($local_val - $sidecar_val) / $sidecar_val) * 100" | bc
}

format_diff() {
    local diff=$1
    local metric_type=$2  # "latency" or "throughput"
    
    if [ "$diff" == "N/A" ]; then
        echo "N/A"
        return
    fi
    
    local abs_diff=$(echo "$diff" | sed 's/-//')
    
    if [ "$metric_type" == "latency" ]; then
        if (( $(echo "$diff < -10" | bc -l) )); then
            echo -e "${GREEN}${diff}% (better)${NC}"
        elif (( $(echo "$diff > 10" | bc -l) )); then
            echo -e "${RED}${diff}% (worse)${NC}"
        else
            echo -e "${YELLOW}${diff}% (similar)${NC}"
        fi
    else  # throughput
        if (( $(echo "$diff > 10" | bc -l) )); then
            echo -e "${GREEN}+${diff}% (better)${NC}"
        elif (( $(echo "$diff < -10" | bc -l) )); then
            echo -e "${RED}${diff}% (worse)${NC}"
        else
            echo -e "${YELLOW}${diff}% (similar)${NC}"
        fi
    fi
}

# Function to compare endpoint
compare_endpoint() {
    local endpoint=$1
    local local_file="$LOCAL_DIR/benchmark_${endpoint}.txt"
    local sidecar_file="$SIDECAR_DIR/benchmark_sidecar_${endpoint}.txt"
    
    if [ ! -f "$local_file" ]; then
        echo -e "${YELLOW}⚠ Local benchmark for $endpoint not found${NC}"
        return
    fi
    
    if [ ! -f "$sidecar_file" ]; then
        echo -e "${YELLOW}⚠ Sidecar benchmark for $endpoint not found${NC}"
        return
    fi
    
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo -e "${BLUE}  Endpoint: $endpoint${NC}"
    echo -e "${BLUE}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
    echo ""
    
    # Extract metrics
    local_rps=$(extract_metric "$local_file" "rps")
    sidecar_rps=$(extract_metric "$sidecar_file" "rps")
    rps_diff=$(calc_diff "$local_rps" "$sidecar_rps")
    
    local_lat_avg=$(extract_metric "$local_file" "latency_avg")
    sidecar_lat_avg=$(extract_metric "$sidecar_file" "latency_avg")
    lat_avg_diff=$(calc_diff "$local_lat_avg" "$sidecar_lat_avg")
    
    local_lat_p50=$(extract_metric "$local_file" "latency_p50")
    sidecar_lat_p50=$(extract_metric "$sidecar_file" "latency_p50")
    lat_p50_diff=$(calc_diff "$local_lat_p50" "$sidecar_lat_p50")
    
    local_lat_p90=$(extract_metric "$local_file" "latency_p90")
    sidecar_lat_p90=$(extract_metric "$sidecar_file" "latency_p90")
    lat_p90_diff=$(calc_diff "$local_lat_p90" "$sidecar_lat_p90")
    
    local_lat_p99=$(extract_metric "$local_file" "latency_p99")
    sidecar_lat_p99=$(extract_metric "$sidecar_file" "latency_p99")
    lat_p99_diff=$(calc_diff "$local_lat_p99" "$sidecar_lat_p99")
    
    # Print comparison table
    printf "%-25s %15s %15s %25s\n" "Metric" "Local API" "Sidecar" "Difference"
    echo "--------------------------------------------------------------------------------"
    printf "%-25s %15s %15s %25s\n" "Throughput (req/sec)" "$local_rps" "$sidecar_rps" "$(format_diff "$rps_diff" "throughput")"
    printf "%-25s %15s %15s %25s\n" "Avg Latency (ms)" "$local_lat_avg" "$sidecar_lat_avg" "$(format_diff "$lat_avg_diff" "latency")"
    printf "%-25s %15s %15s %25s\n" "P50 Latency (ms)" "$local_lat_p50" "$sidecar_lat_p50" "$(format_diff "$lat_p50_diff" "latency")"
    printf "%-25s %15s %15s %25s\n" "P90 Latency (ms)" "$local_lat_p90" "$sidecar_lat_p90" "$(format_diff "$lat_p90_diff" "latency")"
    printf "%-25s %15s %15s %25s\n" "P99 Latency (ms)" "$local_lat_p99" "$sidecar_lat_p99" "$(format_diff "$lat_p99_diff" "latency")"
    echo ""
}

# Get list of endpoints from mapping file
if [ -f "$MAPPING_FILE" ]; then
    ENDPOINTS=$(jq -r '.endpoint_mapping | keys[]' "$MAPPING_FILE")
else
    echo -e "${RED}Error: Mapping file not found: $MAPPING_FILE${NC}"
    echo "Falling back to default endpoints..."
    ENDPOINTS="runtime version"
fi

# Compare each endpoint
for endpoint in $ENDPOINTS; do
    compare_endpoint "$endpoint"
done

echo ""
echo "=================================================="
echo "  Summary"
echo "=================================================="
echo ""
echo "✓ Comparison complete!"
echo ""
echo "Legend:"
echo -e "  ${GREEN}Green${NC} = Local API performs better (>10% improvement)"
echo -e "  ${YELLOW}Yellow${NC} = Similar performance (±10%)"
echo -e "  ${RED}Red${NC} = Sidecar performs better (>10% worse)"
echo ""

