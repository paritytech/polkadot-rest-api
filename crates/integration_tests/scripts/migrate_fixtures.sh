#!/bin/bash

# Fixture Migration Script
# Reorganizes fixtures from flat structure to nested chain/feature structure

set -e

FIXTURES_DIR="$(dirname "$0")/../tests/fixtures"
DRY_RUN=${DRY_RUN:-false}

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Create directory if it doesn't exist
ensure_dir() {
    local dir="$1"
    if [ "$DRY_RUN" = true ]; then
        echo "  Would create directory: $dir"
    else
        mkdir -p "$dir"
    fi
}

# Move file to new location
move_file() {
    local src="$1"
    local dst="$2"
    if [ "$DRY_RUN" = true ]; then
        echo "  $src -> $dst"
    else
        mv "$src" "$dst"
    fi
}

# Determine the feature category for a file
get_feature_category() {
    local filename="$1"

    # Order matters - check more specific patterns first
    case "$filename" in
        blocks_*para_inclusions*)
            echo "paras"
            ;;
        blocks_range*|blocks_header*|blocks_*extrinsics*|blocks_*)
            echo "blocks"
            ;;
        extrinsic_*)
            echo "blocks"
            ;;
        pallets_asset_conversion_*)
            echo "pallets/asset_conversion"
            ;;
        pallets_assets_*)
            echo "pallets/assets"
            ;;
        pallets_balances_*)
            echo "pallets/balances"
            ;;
        pallets_foreign_assets_*)
            echo "pallets/foreign_assets"
            ;;
        pallets_nomination_pools_*)
            echo "pallets/nomination_pools"
            ;;
        pallets_on_going_referenda_*)
            echo "pallets/referenda"
            ;;
        pallets_staking_progress_*|pallets_staking_validators_*)
            echo "pallets/staking"
            ;;
        pallets_pool_assets_*)
            echo "pallets/pool_assets"
            ;;
        pallets_staking_*)
            echo "pallets/staking"
            ;;
        pallets_system_*)
            echo "pallets/system"
            ;;
        pallets_timestamp_*)
            echo "pallets/timestamp"
            ;;
        pallets_*)
            echo "pallets"
            ;;
        runtime_code_*|runtime_metadata_*|runtime_spec_*)
            echo "runtime"
            ;;
        coretime_*)
            echo "coretime"
            ;;
        staking_progress_*|staking_validators_*)
            echo "staking"
            ;;
        rc_blocks_*)
            echo "rc_blocks"
            ;;
        use_rc_block_*)
            echo "use_rc_block"
            ;;
        accounts_*)
            echo "accounts"
            ;;
        *)
            echo "other"
            ;;
    esac
}

# Get new filename (strip the category prefix)
get_new_filename() {
    local filename="$1"
    local category="$2"

    case "$category" in
        blocks)
            # blocks_1000000.json -> 1000000.json
            # blocks_range_1000000-1000002.json -> range_1000000-1000002.json
            # blocks_header_1276963.json -> header_1276963.json
            # extrinsic_11308795_2_with_docs.json -> extrinsic_11308795_2_with_docs.json
            if [[ "$filename" == extrinsic_* ]]; then
                echo "$filename"
            else
                echo "${filename#blocks_}"
            fi
            ;;
        paras)
            # blocks_10293194_para_inclusions.json -> 10293194_inclusions.json
            echo "${filename#blocks_}" | sed 's/para_inclusions/inclusions/'
            ;;
        pallets/referenda)
            # pallets_on_going_referenda_25000000.json -> on_going_25000000.json
            echo "${filename#pallets_}" | sed 's/on_going_referenda_/on_going_/'
            ;;
        pallets/*)
            # pallets_balances_errors_21000000.json -> errors_21000000.json
            # pallets_system_storage_number_20000000.json -> storage_number_20000000.json
            local pallet_name=$(echo "$category" | cut -d'/' -f2)
            echo "${filename#pallets_${pallet_name}_}"
            ;;
        pallets)
            echo "${filename#pallets_}"
            ;;
        runtime)
            echo "${filename#runtime_}"
            ;;
        coretime)
            echo "${filename#coretime_}"
            ;;
        staking)
            # staking_progress_block_12000.json -> progress_12000.json
            echo "${filename#staking_}" | sed 's/_block_/_/'
            ;;
        rc_blocks)
            echo "${filename#rc_blocks_}"
            ;;
        use_rc_block)
            echo "${filename#use_rc_block_}"
            ;;
        accounts)
            echo "${filename#accounts_}"
            ;;
        *)
            echo "$filename"
            ;;
    esac
}

# Process a single chain directory
process_chain() {
    local chain="$1"
    local chain_dir="$FIXTURES_DIR/$chain"

    if [ ! -d "$chain_dir" ]; then
        log_warn "Chain directory not found: $chain_dir"
        return
    fi

    log_info "Processing chain: $chain"

    # Create a temporary directory for the new structure
    local temp_dir="$chain_dir.new"
    if [ "$DRY_RUN" = false ]; then
        rm -rf "$temp_dir"
        mkdir -p "$temp_dir"
    fi

    # Process each file
    for file in "$chain_dir"/*.json; do
        [ -e "$file" ] || continue

        local filename=$(basename "$file")
        local category=$(get_feature_category "$filename")
        local new_filename=$(get_new_filename "$filename" "$category")

        local target_dir="$temp_dir/$category"
        local target_file="$target_dir/$new_filename"

        ensure_dir "$target_dir"

        if [ "$DRY_RUN" = true ]; then
            echo "  $filename -> $category/$new_filename"
        else
            cp "$file" "$target_file"
        fi
    done

    # Replace old directory with new structure
    if [ "$DRY_RUN" = false ]; then
        # Backup old directory
        mv "$chain_dir" "$chain_dir.bak"
        mv "$temp_dir" "$chain_dir"
        log_info "  Backed up old structure to $chain_dir.bak"
    fi
}

# Main
main() {
    if [ "$DRY_RUN" = true ]; then
        log_info "DRY RUN MODE - No files will be modified"
        echo ""
    fi

    log_info "Starting fixture migration..."
    log_info "Fixtures directory: $FIXTURES_DIR"
    echo ""

    # Process each chain
    for chain_dir in "$FIXTURES_DIR"/*/; do
        [ -d "$chain_dir" ] || continue
        chain=$(basename "$chain_dir")

        # Skip if already migrated (has subdirectories)
        if [ -d "$chain_dir/blocks" ] || [ -d "$chain_dir/pallets" ] || [ -d "$chain_dir/runtime" ]; then
            log_warn "Chain $chain appears already migrated, skipping"
            continue
        fi

        process_chain "$chain"
    done

    echo ""
    if [ "$DRY_RUN" = true ]; then
        log_info "Dry run complete. Run with DRY_RUN=false to apply changes."
    else
        log_info "Migration complete!"
        log_info "Old directories backed up with .bak suffix"
        log_info "Review the changes and delete .bak directories when satisfied"
    fi
}

# Handle root-level fixtures that aren't in chain directories
handle_root_fixtures() {
    log_info "Checking for root-level fixtures..."

    for file in "$FIXTURES_DIR"/*.json; do
        [ -e "$file" ] || continue
        local filename=$(basename "$file")
        log_warn "Found root-level fixture: $filename (not migrated)"
    done
}

main
handle_root_fixtures
