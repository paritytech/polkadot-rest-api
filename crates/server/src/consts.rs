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
