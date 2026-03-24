#!/usr/bin/env bash
# Copyright (C) 2026 Parity Technologies (UK) Ltd.
# SPDX-License-Identifier: GPL-3.0-or-later

# Auto-discover endpoints and scenarios from two result directories,
# then run compare_runs.sh for each combination.
#
# Usage:
#   ./compare_all.sh <results-a-dir> <results-b-dir> [--label-a NAME] [--label-b NAME]
#
# Examples:
#   ./compare_all.sh results ../substrate-api-sidecar/results
#   ./compare_all.sh results ../substrate-api-sidecar/results --label-a rest-api --label-b sidecar
#
# The script expects each results directory to have subdirectories per endpoint:
#   results/
#     blocks/
#       rest-api_blocks_20260320_*.json
#     accounts_balance_info/
#       rest-api_accounts_balance_info_20260320_*.json
#
# It discovers which scenarios exist in each endpoint by reading the "scenario"
# field from the JSON files, then generates a comparison report for each
# endpoint+scenario combination.

set -euo pipefail
export LC_ALL=C

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# --- Parse arguments ---
if [ $# -lt 2 ]; then
    echo "Usage: $0 <results-a-dir> <results-b-dir> [--label-a NAME] [--label-b NAME]"
    echo ""
    echo "Examples:"
    echo "  $0 results ../substrate-api-sidecar/results"
    echo "  $0 results ../substrate-api-sidecar/results --label-a rest-api --label-b sidecar"
    exit 1
fi

DIR_A="$1"; shift
DIR_B="$1"; shift

LABEL_A=""
LABEL_B=""
while [ $# -gt 0 ]; do
    case "$1" in
        --label-a) shift; LABEL_A="$1"; shift ;;
        --label-b) shift; LABEL_B="$1"; shift ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

if [ ! -d "$DIR_A" ]; then
    echo "Error: directory not found: $DIR_A"
    exit 1
fi
if [ ! -d "$DIR_B" ]; then
    echo "Error: directory not found: $DIR_B"
    exit 1
fi

# --- Discover endpoints (subdirectories present in BOTH result dirs) ---
ENDPOINTS=()
for d in "$DIR_A"/*/; do
    [ ! -d "$d" ] && continue
    endpoint="$(basename "$d")"
    if [ -d "$DIR_B/$endpoint" ]; then
        ENDPOINTS+=("$endpoint")
    fi
done

if [ ${#ENDPOINTS[@]} -eq 0 ]; then
    echo "No matching endpoint directories found between:"
    echo "  A: $DIR_A"
    echo "  B: $DIR_B"
    exit 1
fi

echo "Found ${#ENDPOINTS[@]} endpoints: ${ENDPOINTS[*]}"
echo ""

# --- For each endpoint, discover scenarios and run compare_runs.sh ---
TOTAL=0
FAILED=0

for endpoint in "${ENDPOINTS[@]}"; do
    # Discover scenarios from JSON files in both dirs
    SCENARIOS_A=()
    for f in "$DIR_A/$endpoint"/*.json; do
        [ ! -f "$f" ] && continue
        [[ "$(basename "$f")" == *_summary_* ]] && continue
        s=$(jq -r '.scenario // ""' "$f" 2>/dev/null)
        [ -n "$s" ] && SCENARIOS_A+=("$s")
    done

    SCENARIOS_B=()
    for f in "$DIR_B/$endpoint"/*.json; do
        [ ! -f "$f" ] && continue
        [[ "$(basename "$f")" == *_summary_* ]] && continue
        s=$(jq -r '.scenario // ""' "$f" 2>/dev/null)
        [ -n "$s" ] && SCENARIOS_B+=("$s")
    done

    # Find scenarios present in both
    COMMON_SCENARIOS=()
    for sa in $(printf '%s\n' "${SCENARIOS_A[@]}" | sort -u); do
        for sb in $(printf '%s\n' "${SCENARIOS_B[@]}" | sort -u); do
            if [ "$sa" = "$sb" ]; then
                COMMON_SCENARIOS+=("$sa")
                break
            fi
        done
    done

    if [ ${#COMMON_SCENARIOS[@]} -eq 0 ]; then
        echo "  $endpoint: no common scenarios, skipping"
        continue
    fi

    # Sort scenarios in severity order
    ORDERED=()
    for s in light_load medium_load heavy_load stress_test; do
        for cs in "${COMMON_SCENARIOS[@]}"; do
            [ "$cs" = "$s" ] && ORDERED+=("$s") && break
        done
    done

    for scenario in "${ORDERED[@]}"; do
        TOTAL=$((TOTAL + 1))
        echo "  $endpoint / $scenario ..."

        LABEL_ARGS=""
        [ -n "$LABEL_A" ] && LABEL_ARGS="LABEL_A=$LABEL_A"
        [ -n "$LABEL_B" ] && LABEL_ARGS="$LABEL_ARGS LABEL_B=$LABEL_B"

        if env $LABEL_ARGS "$SCRIPT_DIR/compare_runs.sh" \
            --a-dir "$DIR_A/$endpoint" \
            --b-dir "$DIR_B/$endpoint" \
            --scenario "$scenario" > /dev/null 2>&1; then
            echo "    done"
        else
            echo "    FAILED"
            FAILED=$((FAILED + 1))
        fi
    done
done

echo ""
echo "Generated $((TOTAL - FAILED))/$TOTAL comparison reports ($FAILED failed)"
