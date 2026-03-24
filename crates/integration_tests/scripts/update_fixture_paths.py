#!/usr/bin/env python3
"""
Script to update fixture paths in test_config.json to match the new nested structure.

Old format: "polkadot/blocks_1000000.json"
New format: "polkadot/blocks/1000000.json"
"""

import json
import re
import sys
from pathlib import Path

def transform_fixture_path(old_path: str) -> str:
    """Transform an old flat fixture path to the new nested structure."""

    # Split into chain and filename
    parts = old_path.split('/', 1)
    if len(parts) != 2:
        return old_path

    chain, filename = parts

    # Define transformation rules (order matters - more specific first)
    transformations = [
        # Blocks - para inclusions go to paras/
        (r'^blocks_(\d+)_para_inclusions(.*)\.json$', r'paras/\1_inclusions\2.json'),

        # Blocks
        (r'^blocks_range_(.*)\.json$', r'blocks/range_\1.json'),
        (r'^blocks_header_(.*)\.json$', r'blocks/header_\1.json'),
        (r'^blocks_(\d+)_extrinsics_raw\.json$', r'blocks/\1_extrinsics_raw.json'),
        (r'^blocks_(\d+)_rc_extrinsics_raw\.json$', r'blocks/\1_rc_extrinsics_raw.json'),
        (r'^blocks_(.*)\.json$', r'blocks/\1.json'),

        # Extrinsics (go to blocks/)
        (r'^extrinsic_(.*)\.json$', r'blocks/extrinsic_\1.json'),

        # RC Blocks
        (r'^rc_blocks_(.*)\.json$', r'rc_blocks/\1.json'),

        # Use RC Block
        (r'^use_rc_block_(.*)\.json$', r'use_rc_block/\1.json'),

        # Pallets - specific pallets
        (r'^pallets_asset_conversion_(.*)\.json$', r'pallets/asset_conversion/\1.json'),
        (r'^pallets_assets_(.*)\.json$', r'pallets/assets/\1.json'),
        (r'^pallets_balances_(.*)\.json$', r'pallets/balances/\1.json'),
        (r'^pallets_foreign_assets_(.*)\.json$', r'pallets/foreign_assets/\1.json'),
        (r'^pallets_nomination_pools_(.*)\.json$', r'pallets/nomination_pools/\1.json'),
        (r'^pallets_on_going_referenda_(.*)\.json$', r'pallets/referenda/on_going_\1.json'),
        (r'^pallets_pool_assets_(.*)\.json$', r'pallets/pool_assets/\1.json'),
        (r'^pallets_staking_(.*)\.json$', r'pallets/staking/\1.json'),
        (r'^pallets_system_(.*)\.json$', r'pallets/system/\1.json'),
        (r'^pallets_timestamp_(.*)\.json$', r'pallets/timestamp/\1.json'),

        # Runtime
        (r'^runtime_code_(.*)\.json$', r'runtime/code_\1.json'),
        (r'^runtime_metadata_(.*)\.json$', r'runtime/metadata_\1.json'),
        (r'^runtime_spec_(.*)\.json$', r'runtime/spec_\1.json'),

        # Coretime
        (r'^coretime_(.*)\.json$', r'coretime/\1.json'),

        # Staking (standalone, not under pallets)
        (r'^staking_progress_block_(\d+)\.json$', r'staking/progress_\1.json'),
        (r'^staking_validators_block_(\d+)\.json$', r'staking/validators_\1.json'),

        # Accounts
        (r'^accounts_(.*)\.json$', r'accounts/\1.json'),
    ]

    for pattern, replacement in transformations:
        match = re.match(pattern, filename)
        if match:
            new_filename = re.sub(pattern, replacement, filename)
            return f"{chain}/{new_filename}"

    # If no transformation matched, return original
    print(f"WARNING: No transformation for: {old_path}", file=sys.stderr)
    return old_path


def update_config(config_path: Path) -> None:
    """Update all fixture_path entries in the config file."""

    with open(config_path, 'r') as f:
        config = json.load(f)

    # Track changes
    changes = []

    # Update historical_tests
    if 'historical_tests' in config:
        for chain, tests in config['historical_tests'].items():
            for test in tests:
                if 'fixture_path' in test:
                    old_path = test['fixture_path']
                    new_path = transform_fixture_path(old_path)
                    if old_path != new_path:
                        changes.append((old_path, new_path))
                        test['fixture_path'] = new_path

    # Write updated config
    with open(config_path, 'w') as f:
        json.dump(config, f, indent=2)
        f.write('\n')  # Add trailing newline

    # Print summary
    print(f"Updated {len(changes)} fixture paths:")
    for old, new in changes[:10]:
        print(f"  {old}")
        print(f"    -> {new}")
    if len(changes) > 10:
        print(f"  ... and {len(changes) - 10} more")


def main():
    script_dir = Path(__file__).parent
    config_path = script_dir.parent / 'tests' / 'config' / 'test_config.json'

    if not config_path.exists():
        print(f"Error: Config file not found: {config_path}", file=sys.stderr)
        sys.exit(1)

    print(f"Updating fixture paths in: {config_path}")
    update_config(config_path)
    print("Done!")


if __name__ == '__main__':
    main()
