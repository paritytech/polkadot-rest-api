// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

/// Migration boundaries for AssetHub staking migration
/// These define when staking migrated from relay chain to AssetHub
#[derive(Debug, Clone)]
pub struct MigrationBoundaries {
    pub relay_chain_last_era: u32,
    pub asset_hub_first_era: u32,
    pub asset_hub_migration_started_at: u32,
    pub asset_hub_migration_ended_at: u32,
    pub relay_migration_started_at: u32,
    pub relay_migration_ended_at: u32,
}

/// Static migration boundaries for known chains
pub const MIGRATION_BOUNDARIES: &[(&str, MigrationBoundaries)] = &[
    (
        "westmint",
        MigrationBoundaries {
            relay_chain_last_era: 9297,
            asset_hub_first_era: 9297,
            asset_hub_migration_started_at: 11716733,
            asset_hub_migration_ended_at: 11736597,
            relay_migration_started_at: 26041702,
            relay_migration_ended_at: 26071771,
        },
    ),
    (
        "asset-hub-paseo",
        MigrationBoundaries {
            relay_chain_last_era: 2218,
            asset_hub_first_era: 2218,
            asset_hub_migration_started_at: 2593897,
            asset_hub_migration_ended_at: 2594172,
            relay_migration_started_at: 7926930,
            relay_migration_ended_at: 7927225,
        },
    ),
    (
        "statemine",
        MigrationBoundaries {
            relay_chain_last_era: 8662,
            asset_hub_first_era: 8662,
            asset_hub_migration_started_at: 11150168,
            asset_hub_migration_ended_at: 11151931,
            relay_migration_started_at: 30423691,
            relay_migration_ended_at: 30425590,
        },
    ),
    (
        "statemint",
        MigrationBoundaries {
            relay_chain_last_era: 1981,
            asset_hub_first_era: 1981,
            asset_hub_migration_started_at: 10254470,
            asset_hub_migration_ended_at: 10259208,
            relay_migration_started_at: 28490502,
            relay_migration_ended_at: 28495696,
        },
    ),
];

/// Mapping from relay chain spec names to their AssetHub spec names
pub const RELAY_TO_SPEC_MAPPING: &[(&str, &str)] = &[
    ("polkadot", "statemint"),
    ("kusama", "statemine"),
    ("westend", "westmint"),
    ("paseo", "asset-hub-paseo"),
];

/// Find migration boundaries by spec name
pub fn get_migration_boundaries(spec_name: &str) -> Option<&MigrationBoundaries> {
    MIGRATION_BOUNDARIES
        .iter()
        .find(|(name, _)| *name == spec_name)
        .map(|(_, boundaries)| boundaries)
}

/// Find Asset Hub spec name by relay chain spec name
pub fn get_asset_hub_spec_name(relay_spec_name: &str) -> Option<&str> {
    RELAY_TO_SPEC_MAPPING
        .iter()
        .find(|(relay, _)| *relay == relay_spec_name)
        .map(|(_, asset_hub)| *asset_hub)
}

// ================================================================================================
// Bad Staking Blocks
// ================================================================================================

/// A contiguous range of bad staking blocks (inclusive on both ends).
///
/// During the staking migration from relay chain to Asset Hub, certain block ranges
/// had unreliable staking data. Queries at these blocks should be rejected to avoid
/// returning incorrect information.
pub struct BadStakingBlockRange {
    pub start: u64,
    pub end: u64,
}

/// Bad staking block ranges per chain spec name.
///
/// Currently only westmint (Westend Asset Hub) has known bad blocks from
/// post-migration interruptions (blocks 11716733 through 11746809).
const BAD_STAKING_BLOCKS: &[(&str, &[BadStakingBlockRange])] = &[(
    "westmint",
    &[BadStakingBlockRange {
        start: 11716733,
        end: 11746809,
    }],
)];

/// Check if a given block number is a known bad staking block for the given chain.
pub fn is_bad_staking_block(spec_name: &str, block_number: u64) -> bool {
    for (name, ranges) in BAD_STAKING_BLOCKS {
        if *name == spec_name {
            for range in *ranges {
                if block_number >= range.start && block_number <= range.end {
                    return true;
                }
            }
        }
    }
    false
}

/// Get a user-friendly display name for a chain spec name.
pub fn get_chain_display_name(spec_name: &str) -> &str {
    match spec_name {
        "westmint" => "Westend Asset Hub",
        "statemine" => "Kusama Asset Hub",
        "statemint" => "Polkadot Asset Hub",
        "asset-hub-paseo" => "Paseo Asset Hub",
        _ => spec_name,
    }
}
