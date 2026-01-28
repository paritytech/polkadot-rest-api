#!/bin/bash
#
# Start the polkadot-rest-api server with fallback RPC URLs.
# If the primary RPC is down, it will try alternative URLs.
#
# Usage: ./scripts/start-server-with-fallback.sh <chain>
#
# Arguments:
#   chain - One of: polkadot, kusama, asset-hub-polkadot, asset-hub-kusama, westend
#
# Environment variables (optional):
#   SERVER_BINARY - Path to the server binary (default: ./target/release/polkadot-rest-api)
#   HEALTH_TIMEOUT - Seconds to wait for health check (default: 60)
#   API_PORT - Port the server runs on (default: 8080)
#

set -e

CHAIN=$1
SERVER_BINARY="${SERVER_BINARY:-./target/release/polkadot-rest-api}"
HEALTH_TIMEOUT="${HEALTH_TIMEOUT:-60}"
API_PORT="${API_PORT:-8080}"
LOG_FILE="${CHAIN}-server.log"
PID_FILE="${CHAIN}-server.pid"

if [ -z "$CHAIN" ]; then
    echo "Usage: $0 <chain>"
    echo "Chains: polkadot, kusama, asset-hub-polkadot, asset-hub-kusama, westend"
    exit 1
fi

# Define RPC URLs per chain (primary first, then fallbacks)
case $CHAIN in
    polkadot)
        RPC_URLS=(
            # "wss://rpc.polkadot.io" NOTE: Do not use parity hosted RPC nodes because of rate limiting
            "wss://rpc.ibp.network/polkadot"
            "wss://polkadot.dotters.network"
            "wss://polkadot-rpc.dwellir.com"
        )
        ;;
    kusama)
        RPC_URLS=(
            "wss://rpc.ibp.network/kusama"
            "wss://kusama.dotters.network"
            "wss://kusama-rpc.dwellir.com"
        )
        ;;
    asset-hub-polkadot)
        RPC_URLS=(
            "wss://sys.ibp.network/asset-hub-polkadot"
            "wss://asset-hub-polkadot-rpc.dwellir.com"
            "wss://asset-hub-polkadot.dotters.network"
        )
        RELAY_URL="wss://rpc.ibp.network/polkadot"
        ;;
    asset-hub-kusama)
        RPC_URLS=(
            "wss://sys.ibp.network/asset-hub-kusama"
            "wss://asset-hub-kusama.dotters.network"
            "wss://asset-hub-kusama-rpc.n.dwellir.com"
        )
        RELAY_URL="wss://rpc.ibp.network/kusama"
        ;;
    westend)
        RPC_URLS=(
            "wss://westend-rpc.dwellir.com"
            "wss://westend-rpc-tn.dwellir.com"
        )
        ;;
    *)
        echo "Unknown chain: $CHAIN"
        echo "Supported chains: polkadot, kusama, asset-hub-polkadot, asset-hub-kusama, westend"
        exit 1
        ;;
esac

echo "========================================"
echo "Starting server for: $CHAIN"
echo "========================================"

# Function to check if server is healthy
check_health() {
    curl -sf "http://localhost:${API_PORT}/v1/health" > /dev/null 2>&1
}

# Function to stop server if running
stop_server() {
    if [ -f "$PID_FILE" ]; then
        local pid=$(cat "$PID_FILE")
        if kill -0 "$pid" 2>/dev/null; then
            echo "  Stopping server (PID: $pid)..."
            kill "$pid" 2>/dev/null || true
            sleep 2
            # Force kill if still running
            if kill -0 "$pid" 2>/dev/null; then
                kill -9 "$pid" 2>/dev/null || true
            fi
        fi
        rm -f "$PID_FILE"
    fi
}

# Try each RPC URL
for url in "${RPC_URLS[@]}"; do
    echo ""
    echo "Trying RPC: $url"
    echo "----------------------------------------"

    # Make sure any previous server is stopped
    stop_server

    # Start server with this RPC URL (and relay chain URL if configured)
    if [ -n "$RELAY_URL" ]; then
        MULTI_CHAIN_JSON="[{\"url\":\"${RELAY_URL}\",\"type\":\"relay\"}]"
        RUST_LOG=info SAS_SUBSTRATE_URL="$url" SAS_SUBSTRATE_MULTI_CHAIN_URL="$MULTI_CHAIN_JSON" "$SERVER_BINARY" > "$LOG_FILE" 2>&1 &
    else
        RUST_LOG=info SAS_SUBSTRATE_URL="$url" "$SERVER_BINARY" > "$LOG_FILE" 2>&1 &
    fi
    echo $! > "$PID_FILE"

    echo "  Server started (PID: $(cat $PID_FILE))"
    echo "  Waiting for health check (timeout: ${HEALTH_TIMEOUT}s)..."

    # Wait for server to be ready
    for i in $(seq 1 $HEALTH_TIMEOUT); do
        if check_health; then
            echo ""
            echo "========================================"
            echo "SUCCESS: $CHAIN server is ready"
            echo "  RPC: $url"
            if [ -n "$RELAY_URL" ]; then
                echo "  Relay Chain: $RELAY_URL"
            fi
            echo "  API: http://localhost:${API_PORT}"
            echo "  PID: $(cat $PID_FILE)"
            echo "  Log: $LOG_FILE"
            echo "========================================"
            exit 0
        fi

        # Check if server process is still running
        if ! kill -0 "$(cat $PID_FILE)" 2>/dev/null; then
            echo "  Server process died unexpectedly"
            echo "  Last 20 lines of log:"
            tail -20 "$LOG_FILE" | sed 's/^/    /'
            break
        fi

        # Progress indicator every 10 seconds
        if [ $((i % 10)) -eq 0 ]; then
            echo "  Still waiting... (${i}s / ${HEALTH_TIMEOUT}s)"
        fi

        sleep 1
    done

    echo "  Failed to connect with $url"
    echo "  Last 10 lines of log:"
    tail -10 "$LOG_FILE" | sed 's/^/    /'
done

# All URLs failed
echo ""
echo "========================================"
echo "FAILED: All RPC URLs failed for $CHAIN"
echo "========================================"
echo ""
echo "Tried URLs:"
for url in "${RPC_URLS[@]}"; do
    echo "  - $url"
done
echo ""
echo "Full server log:"
cat "$LOG_FILE"

stop_server
exit 1
