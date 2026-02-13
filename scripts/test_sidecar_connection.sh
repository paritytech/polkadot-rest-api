#!/bin/bash
# Copyright (C) 2026 Parity Technologies (UK) Ltd.
# SPDX-License-Identifier: GPL-3.0-or-later


set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
MAPPING_FILE="$PROJECT_ROOT/sidecar_mapping.json"

echo "🔍 Testing Sidecar Connectivity"
echo "================================"
echo ""

# Get Sidecar server config
SERVER_HOST=$(jq -r '.sidecar_server.host' "$MAPPING_FILE")
SERVER_PORT=$(jq -r '.sidecar_server.port' "$MAPPING_FILE")
PROTOCOL=$(jq -r '.sidecar_server.protocol' "$MAPPING_FILE")

BASE_URL="${PROTOCOL}://${SERVER_HOST}:${SERVER_PORT}"

echo "Testing against: $BASE_URL"
echo ""

# Test each endpoint
ENDPOINTS=$(jq -r '.endpoint_mapping | to_entries[] | "\(.key):\(.value.sidecar_endpoint)"' "$MAPPING_FILE")

ALL_PASSED=true

while IFS=: read -r name endpoint; do
    echo -n "Testing $name ($endpoint)... "
    
    FULL_URL="${BASE_URL}${endpoint}"
    
    if HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" -m 10 "$FULL_URL"); then
        if [ "$HTTP_CODE" = "200" ]; then
            echo "✅ OK (HTTP $HTTP_CODE)"
        else
            echo "⚠️  Warning (HTTP $HTTP_CODE)"
            ALL_PASSED=false
        fi
    else
        echo "❌ Failed (connection error)"
        ALL_PASSED=false
    fi
done <<< "$ENDPOINTS"

echo ""

if [ "$ALL_PASSED" = true ]; then
    echo "✅ All endpoints are accessible!"
    echo ""
    echo "You can now run benchmarks:"
    echo "  ./scripts/run_comparison.sh light_load development"
    exit 0
else
    echo "⚠️  Some endpoints had issues"
    echo ""
    echo "This might affect benchmark results."
    echo "Check if Sidecar instance is accessible:"
    echo "  curl $BASE_URL/runtime/spec"
    exit 1
fi

