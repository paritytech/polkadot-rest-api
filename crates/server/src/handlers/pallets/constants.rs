pub struct AssetHubBabeParams {
    /// Number of slots per epoch (session)
    pub epoch_duration: u64,
    /// The genesis slot number for the chain
    pub genesis_slot: u64,
    /// Slot duration in milliseconds
    pub slot_duration_ms: u64,
}

/// BABE parameters for Asset Hub Polkadot
pub const ASSET_HUB_POLKADOT_BABE_PARAMS: AssetHubBabeParams = AssetHubBabeParams {
    epoch_duration: 2400,
    genesis_slot: 265084563,
    slot_duration_ms: 6000,
};

/// BABE parameters for Asset Hub Kusama
pub const ASSET_HUB_KUSAMA_BABE_PARAMS: AssetHubBabeParams = AssetHubBabeParams {
    epoch_duration: 600,
    genesis_slot: 262493679,
    slot_duration_ms: 6000,
};

/// BABE parameters for Asset Hub Westend
pub const ASSET_HUB_WESTEND_BABE_PARAMS: AssetHubBabeParams = AssetHubBabeParams {
    epoch_duration: 600,
    genesis_slot: 264379767,
    slot_duration_ms: 6000,
};

/// Block range with known staking issues on Asset Hub Westend
pub const ASSET_HUB_WESTEND_BAD_STAKING_BLOCKS: std::ops::RangeInclusive<u64> = 11716733..=11746809;

/// BABE parameters for Asset Hub Paseo
pub const ASSET_HUB_PASEO_BABE_PARAMS: AssetHubBabeParams = AssetHubBabeParams {
    epoch_duration: 600,
    genesis_slot: 284730328,
    slot_duration_ms: 6000,
};

/// BABE epoch duration for Polkadot relay chain (slots per epoch)
pub const POLKADOT_EPOCH_DURATION: u64 = 2400;

/// Default BABE epoch duration for other relay chains (slots per epoch)
pub const DEFAULT_EPOCH_DURATION: u64 = 600;

/// Election lookahead divisor for Polkadot and its Asset Hub
pub const POLKADOT_ELECTION_LOOKAHEAD_DIVISOR: u64 = 16;

/// Default election lookahead divisor for other chains
pub const DEFAULT_ELECTION_LOOKAHEAD_DIVISOR: u64 = 4;

/// Get BABE parameters for an Asset Hub chain by spec name.
///
/// Returns `None` if the spec name is not recognized as an Asset Hub chain.
pub fn get_asset_hub_babe_params(spec_name: &str) -> Option<AssetHubBabeParams> {
    match spec_name {
        "statemint" | "asset-hub-polkadot" => Some(ASSET_HUB_POLKADOT_BABE_PARAMS),
        "statemine" | "asset-hub-kusama" => Some(ASSET_HUB_KUSAMA_BABE_PARAMS),
        "westmint" | "asset-hub-westend" => Some(ASSET_HUB_WESTEND_BABE_PARAMS),
        "asset-hub-paseo" => Some(ASSET_HUB_PASEO_BABE_PARAMS),
        _ => None,
    }
}

/// Check if a block is in a known bad range for staking data.
///
/// Some blocks have known issues with staking data due to runtime bugs
/// or upgrade issues.
pub fn is_bad_staking_block(spec_name: &str, block_number: u64) -> bool {
    match spec_name {
        "westmint" | "asset-hub-westend" => {
            ASSET_HUB_WESTEND_BAD_STAKING_BLOCKS.contains(&block_number)
        }
        _ => false,
    }
}

/// Get the BABE epoch duration for a relay chain by spec name.
pub fn get_babe_epoch_duration(spec_name: &str) -> u64 {
    match spec_name {
        "polkadot" => POLKADOT_EPOCH_DURATION,
        _ => DEFAULT_EPOCH_DURATION,
    }
}

/// Derive the election lookahead in blocks for a chain.
///
/// The election lookahead determines how many blocks before the end of a session
/// the election window opens.
pub fn derive_election_lookahead(spec_name: &str, epoch_duration: u64) -> u64 {
    let divisor = match spec_name {
        "polkadot" | "statemint" | "asset-hub-polkadot" => POLKADOT_ELECTION_LOOKAHEAD_DIVISOR,
        _ => DEFAULT_ELECTION_LOOKAHEAD_DIVISOR,
    };
    epoch_duration / divisor
}
