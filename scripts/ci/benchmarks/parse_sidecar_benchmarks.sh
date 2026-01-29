#!/bin/bash

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
# Uses explicit mapping for known endpoints, falls back to automatic conversion
endpoint_to_filename() {
    local endpoint=$1
    
    # Explicit mapping for endpoints that have local benchmarks
    case "$endpoint" in
        "/node/version")
            echo "node_version"
            return
            ;;
        "/runtime/spec")
            echo "runtime"
            return
            ;;
        "/node/network")
            echo "node_network"
            return
            ;;
        "/node/transaction-pool")
            echo "node_transaction_pool"
            return
            ;;
    esac
    
    # Automatic conversion for other endpoints
    # Remove leading slash
    endpoint=${endpoint#/}
    # Replace {param} with nothing
    endpoint=$(echo "$endpoint" | sed 's/{[^}]*}//g')
    # Replace / with -
    endpoint=$(echo "$endpoint" | sed 's/\//-/g')
    # Remove multiple consecutive dashes
    endpoint=$(echo "$endpoint" | sed 's/--*/-/g')
    # Remove leading/trailing dashes
    endpoint=$(echo "$endpoint" | sed 's/^-\|-$//g')
    
    # Handle special cases for duplicate names
    case "$endpoint" in
        "pallets-storage")
            # Check if this is the nested version by looking at the original endpoint
            if [[ "$1" == *"/storage/"* ]]; then
                echo "pallets-storage-item"
            else
                echo "pallets-storage"
            fi
            return
            ;;
        "pallets-errors")
            if [[ "$1" == *"/errors/"* ]]; then
                echo "pallets-errors-item"
            else
                echo "pallets-errors"
            fi
            return
            ;;
    esac
    
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

