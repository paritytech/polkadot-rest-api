#!/bin/bash
# Script to update integration test fixtures using the local API connected to public nodes

set -e

echo "================================================================"
echo "Fixture Update Script"
echo "================================================================"
echo ""

# Configuration
API_PORT="${SAS_EXPRESS_PORT:-8080}"
API_URL="http://localhost:${API_PORT}"

# Build the server once
echo "Building API server..."
cargo build --release --package server
echo "  ✓ Built"
echo ""

# Function to update fixtures for a specific chain
update_chain_fixtures() {
    local CHAIN_NAME=$1
    local RPC_URL=$2
    
    echo "================================================================"
    echo "Updating ${CHAIN_NAME} fixtures"
    echo "================================================================"
    echo "  RPC URL: ${RPC_URL}"
    echo ""
    
    # Start the server in the background
    echo "Starting API server (connected to ${CHAIN_NAME})..."
    SAS_SUBSTRATE_URL="${RPC_URL}" RUST_LOG=info \
      ./target/release/polkadot-rest-api > /tmp/polkadot-api-fixture-update-${CHAIN_NAME}.log 2>&1 &
    local SERVER_PID=$!
    
    echo "  Server PID: ${SERVER_PID}"
    echo "  Log file: /tmp/polkadot-api-fixture-update-${CHAIN_NAME}.log"
    
    # Wait for server to be ready
    echo ""
    echo "Waiting for API to be ready..."
    local max_wait=30
    local waited=0
    while [ $waited -lt $max_wait ]; do
        if curl -s "${API_URL}/v1/health" > /dev/null 2>&1; then
            echo "  ✓ API is ready"
            break
        fi
        sleep 1
        waited=$((waited + 1))
        echo -n "."
    done
    echo ""
    if [ $waited -eq $max_wait ]; then
        echo "  ✗ API did not become ready after ${max_wait} seconds"
        kill ${SERVER_PID} 2>/dev/null || true
        exit 1
    fi
    
    # Run the fixture updater for this chain
    echo ""
    echo "Running fixture updater for ${CHAIN_NAME}..."
    echo ""
    
    API_URL="${API_URL}" cargo run --package integration_tests --bin update_fixtures -- ${CHAIN_NAME}
    
    # Stop the server
    echo ""
    echo "Stopping API server (PID: ${SERVER_PID})..."
    kill ${SERVER_PID} 2>/dev/null || true
    wait ${SERVER_PID} 2>/dev/null || true
    echo "  ✓ Server stopped"
    echo ""
}

# Update Polkadot fixtures
update_chain_fixtures "polkadot" "wss://rpc.polkadot.io"

# Update Kusama fixtures
update_chain_fixtures "kusama" "wss://kusama-rpc.polkadot.io"

# Update Asset Hub Polkadot fixtures
update_chain_fixtures "asset-hub-polkadot" "wss://polkadot-asset-hub-rpc.polkadot.io"

# Update Asset Hub Kusama fixtures
update_chain_fixtures "asset-hub-kusama" "wss://kusama-asset-hub-rpc.polkadot.io"

echo "================================================================"
echo "✓ All fixtures updated successfully!"
echo "================================================================"
echo ""
echo "Updated fixtures for all chains (see test_config.json for details)"
echo ""

