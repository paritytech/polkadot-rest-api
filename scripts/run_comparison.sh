#!/bin/bash

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

SCENARIO="${1:-light_load}"
HARDWARE_PROFILE="${2:-development}"

echo "🚀 Running Benchmark Comparison"
echo "================================"
echo "Scenario: $SCENARIO"
echo "Hardware Profile: $HARDWARE_PROFILE"
echo ""

# Create artifacts directories
mkdir -p "$PROJECT_ROOT/artifacts"
mkdir -p "$PROJECT_ROOT/artifacts_sidecar"

# Check if local server is running
echo "📡 Checking local server..."
if ! curl -s http://localhost:8080/health > /dev/null 2>&1; then
    echo "❌ Local server not running on localhost:8080"
    exit 1
fi
echo "✅ Local server is running"
echo ""

# Run local benchmarks
echo "🔧 Running local API benchmarks..."
for init_script in "$PROJECT_ROOT"/benchmarks/*/init.sh; do
    if [ -f "$init_script" ]; then
        ENDPOINT_DIR=$(dirname "$init_script")
        ENDPOINT_NAME=$(basename "$ENDPOINT_DIR")
        
        echo "  → $ENDPOINT_NAME"
        cd "$ENDPOINT_DIR"
        bash init.sh "$SCENARIO" "$HARDWARE_PROFILE" > "$PROJECT_ROOT/artifacts/benchmark_${ENDPOINT_NAME}.txt" 2>&1
        cd - > /dev/null
    fi
done
echo ""

# Check Sidecar connectivity
echo "🌐 Checking Sidecar connectivity..."
if ! curl -s -m 10 https://polkadot-public-sidecar.parity-chains.parity.io/runtime/spec > /dev/null 2>&1; then
    echo "❌ Cannot reach Sidecar instance"
    echo "Skipping Sidecar benchmarks"
    exit 1
fi
echo "✅ Sidecar is reachable"
echo ""

# Run Sidecar benchmarks
echo "🔧 Running Sidecar benchmarks..."
for init_script in "$PROJECT_ROOT"/benchmarks_sidecar/*/init.sh; do
    if [ -f "$init_script" ]; then
        ENDPOINT_DIR=$(dirname "$init_script")
        ENDPOINT_NAME=$(basename "$ENDPOINT_DIR")
        
        echo "  → $ENDPOINT_NAME"
        cd "$ENDPOINT_DIR"
        bash init.sh "$SCENARIO" "$HARDWARE_PROFILE" > "$PROJECT_ROOT/artifacts_sidecar/benchmark_sidecar_${ENDPOINT_NAME}.txt" 2>&1
        cd - > /dev/null
    fi
done
echo ""

# Generate comparison
echo "📊 Generating comparison report..."
cd "$PROJECT_ROOT"
./scripts/ci/benchmarks/compare_results.sh artifacts artifacts_sidecar | tee comparison_report.txt

echo ""
echo "✅ Comparison complete!"
echo ""
echo "Results saved to:"
echo "  - Local: artifacts/"
echo "  - Sidecar: artifacts_sidecar/"
echo "  - Comparison: comparison_report.txt"

