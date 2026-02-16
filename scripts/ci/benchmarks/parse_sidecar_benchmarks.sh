#!/bin/bash
# Copyright (C) 2026 Parity Technologies (UK) Ltd.
# SPDX-License-Identifier: GPL-3.0-or-later


# Script to parse sidecar-benchmarks.txt and generate individual benchmark files
# Usage: ./parse_sidecar_benchmarks.sh <sidecar_benchmarks_file> <output_dir>

set -e

SIDECAR_FILE="${1:-sidecar-benchmarks.txt}"
OUTPUT_DIR="${2:-artifacts_sidecar}"

if [ ! -f "$SIDECAR_FILE" ]; then
    echo "Error: Sidecar benchmarks file not found: $SIDECAR_FILE"
    exit 1
fi

# Create output directory
mkdir -p "$OUTPUT_DIR"

# Function to convert endpoint path to filename
# Uses explicit mapping for all known endpoints, falls back to automatic conversion
endpoint_to_filename() {
    local endpoint=$1

    # Explicit mapping for all sidecar endpoints to local benchmark names
    case "$endpoint" in
        "/accounts/{accountId}/balance-info")           echo "accounts_balance_info" ; return ;;
        "/accounts/{accountId}/vesting-info")            echo "accounts_vesting_info" ; return ;;
        "/accounts/{accountId}/staking-info")            echo "accounts_staking_info" ; return ;;
        "/accounts/{accountId}/staking-payouts")         echo "accounts_staking_payouts" ; return ;;
        "/accounts/{accountId}/validate")                echo "accounts_validate" ; return ;;
        "/accounts/{accountId}/convert")                 echo "accounts_convert" ; return ;;
        "/blocks/{blockId}")                             echo "blocks" ; return ;;
        "/blocks/{blockId}/header")                      echo "blocks_header" ; return ;;
        "/blocks/{blockId}/extrinsics/{extrinsicIndex}") echo "blocks_extrinsics" ; return ;;
        "/blocks/head")                                  echo "blocks_head" ; return ;;
        "/blocks/head/header")                           echo "blocks_head_header" ; return ;;
        "/pallets/staking/progress")                     echo "pallets_staking_progress" ; return ;;
        "/pallets/{palletId}/storage")                   echo "pallets_storage" ; return ;;
        "/pallets/{palletId}/storage/{storageItemId}")   echo "pallets_storage_item" ; return ;;
        "/pallets/{palletId}/errors")                    echo "pallets_errors" ; return ;;
        "/pallets/{palletId}/errors/{errorItemId}")      echo "pallets_errors_item" ; return ;;
        "/pallets/nomination-pools/info")                echo "pallets_nomination_pools_info" ; return ;;
        "/pallets/nomination-pools/{poolId}")            echo "pallets_nomination_pools_id" ; return ;;
        "/pallets/staking/validators")                   echo "pallets_staking_validators" ; return ;;
        "/paras")                                        echo "paras" ; return ;;
        "/paras/leases/current")                         echo "paras_leases_current" ; return ;;
        "/paras/auctions/current")                       echo "paras_auctions_current" ; return ;;
        "/paras/crowdloans")                             echo "paras_crowdloans" ; return ;;
        "/paras/{paraId}/crowdloan-info")                echo "paras_crowdloan_info" ; return ;;
        "/paras/{paraId}/lease-info")                    echo "paras_lease_info" ; return ;;
        "/node/network")                                 echo "node_network" ; return ;;
        "/node/transaction-pool")                        echo "node_transaction_pool" ; return ;;
        "/node/version")                                 echo "node_version" ; return ;;
        "/runtime/spec")                                 echo "runtime" ; return ;;
        "/transaction/material")                         echo "transaction_material" ; return ;;
    esac

    # Fallback: automatic conversion using underscores
    # Remove leading slash
    endpoint=${endpoint#/}
    # Replace {param} with nothing
    endpoint=$(echo "$endpoint" | sed 's/{[^}]*}//g')
    # Replace / and - with _
    endpoint=$(echo "$endpoint" | sed 's/[\/\-]/_/g')
    # Remove multiple consecutive underscores
    endpoint=$(echo "$endpoint" | sed 's/__*/_/g')
    # Remove leading/trailing underscores
    endpoint=$(echo "$endpoint" | sed 's/^_//;s/_$//')

    echo "$endpoint"
}

# Parse the file
current_endpoint=""
current_content=""
in_section=false
newline=""

while IFS= read -r line || [ -n "$line" ]; do
    # Check if this is a new endpoint section
    if [[ "$line" =~ ^Result\ of\ (.+):\ .*$ ]]; then
        # Save previous endpoint if exists
        if [ -n "$current_endpoint" ] && [ "$in_section" = true ]; then
            filename=$(endpoint_to_filename "$current_endpoint")
            output_file="$OUTPUT_DIR/benchmark_sidecar_${filename}.txt"
            echo "Creating: $output_file for endpoint: $current_endpoint"
            printf "%s" "$current_content" > "$output_file"
        fi
        
        # Start new section
        current_endpoint="${BASH_REMATCH[1]}"
        current_content=""
        in_section=true
        newline=""
        # Add the header line
        current_content="Result of ${current_endpoint}: ↓"
    elif [ "$in_section" = true ]; then
        # Add line to current content
        current_content="${current_content}${newline}${line}"
        newline=$'\n'
    fi
done < "$SIDECAR_FILE"

# Save last endpoint
if [ -n "$current_endpoint" ] && [ "$in_section" = true ]; then
    filename=$(endpoint_to_filename "$current_endpoint")
    output_file="$OUTPUT_DIR/benchmark_sidecar_${filename}.txt"
    echo "Creating: $output_file for endpoint: $current_endpoint"
    printf "%s" "$current_content" > "$output_file"
fi

echo ""
echo "✓ Parsed $SIDECAR_FILE and created benchmark files in $OUTPUT_DIR"

