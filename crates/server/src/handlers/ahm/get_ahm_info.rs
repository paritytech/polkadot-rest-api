use crate::consts::{get_asset_hub_spec_name, get_migration_boundaries};
use crate::state::AppState;
use crate::utils;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use config::ChainType;
use parity_scale_codec::Decode;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sp_core::twox_128;
use subxt_rpcs::rpc_params;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GetAhmInfoError {
    #[error("Invalid chain specName. Can't map specName to asset hub spec")]
    InvalidChainSpec,

    #[error("No migration data available for chain: {0}")]
    NoMigrationData(String),

    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] crate::utils::BlockIdParseError),

    #[error("Block resolution failed")]
    BlockResolveFailed(#[from] crate::utils::BlockResolveError),
}

impl IntoResponse for GetAhmInfoError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            GetAhmInfoError::InvalidChainSpec | GetAhmInfoError::NoMigrationData(_) => {
                (StatusCode::NOT_FOUND, self.to_string())
            }
            GetAhmInfoError::InvalidBlockParam(_) | GetAhmInfoError::BlockResolveFailed(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

#[derive(Debug, Deserialize)]
pub struct AhmInfoParams {
    pub at: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AhmStartEndBlocks {
    #[serde(serialize_with = "serialize_option_u32_as_string")]
    pub start_block: Option<u32>,
    #[serde(serialize_with = "serialize_option_u32_as_string")]
    pub end_block: Option<u32>,
}

/// Serialize Option<u32> as Option<String> to match sidecar's behavior
fn serialize_option_u32_as_string<S>(value: &Option<u32>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match value {
        Some(v) => serializer.serialize_some(&v.to_string()),
        None => serializer.serialize_none(),
    }
}

#[derive(Debug, Serialize)]
pub struct AhmInfoResponse {
    pub relay: AhmStartEndBlocks,
    #[serde(rename = "assetHub")]
    pub asset_hub: AhmStartEndBlocks,
}

/// Get Asset Hub Migration information
///
/// This endpoint returns information about the Asset Hub migration, including
/// start and end blocks for both relay chain and Asset Hub.
///
/// Query Parameters:
/// - `at` (optional): Block at which to retrieve AHM information. Can be a block height or block hash. Defaults to most recent block.
///
/// Returns:
/// - Information about migration boundaries for relay and asset hub
pub async fn ahm_info(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<AhmInfoParams>,
) -> Result<Json<AhmInfoResponse>, GetAhmInfoError> {
    // Parse the block identifier
    let block_id = params
        .at
        .map(|s| s.parse::<utils::BlockId>())
        .transpose()?;

    // Resolve the block
    let resolved_block = utils::resolve_block(&state, block_id).await?;
    let block_hash = resolved_block.hash;

    // Determine if we're connected to a relay chain or asset hub
    let (relay, asset_hub) = match state.chain_info.chain_type {
        ChainType::AssetHub => {
            handle_from_asset_hub(&state, Some(&block_hash)).await?
        }
        ChainType::Relay => {
            handle_from_relay(&state, Some(&block_hash)).await?
        }
        _ => {
            return Err(GetAhmInfoError::NoMigrationData(
                state.chain_info.spec_name.clone(),
            ));
        }
    };

    Ok(Json(AhmInfoResponse {
        relay,
        asset_hub,
    }))
}

/// Handle AHM info when connected to Asset Hub
async fn handle_from_asset_hub(
    state: &AppState,
    at_hash: Option<&str>,
) -> Result<(AhmStartEndBlocks, AhmStartEndBlocks), GetAhmInfoError> {
    let spec_name = &state.chain_info.spec_name;

    // Try to find migration boundaries for this spec
    if let Some(boundaries) = get_migration_boundaries(spec_name.as_str()) {
        return Ok((
            AhmStartEndBlocks {
                start_block: Some(boundaries.relay_migration_started_at),
                end_block: Some(boundaries.relay_migration_ended_at),
            },
            AhmStartEndBlocks {
                start_block: Some(boundaries.asset_hub_migration_started_at),
                end_block: Some(boundaries.asset_hub_migration_ended_at),
            },
        ));
    }

    // No static boundaries found, query AhMigrator pallet storage
    let ah_start = query_storage_u32(state, "AhMigrator", "MigrationStartBlock", at_hash).await;
    let ah_end = query_storage_u32(state, "AhMigrator", "MigrationEndBlock", at_hash).await;

    Ok((
        AhmStartEndBlocks {
            start_block: None,
            end_block: None,
        },
        AhmStartEndBlocks {
            start_block: ah_start,
            end_block: ah_end,
        },
    ))
}

/// Handle AHM info when connected to Relay Chain
async fn handle_from_relay(
    state: &AppState,
    at_hash: Option<&str>,
) -> Result<(AhmStartEndBlocks, AhmStartEndBlocks), GetAhmInfoError> {
    let spec_name = &state.chain_info.spec_name;

    // Map relay spec name to asset hub spec name
    let asset_hub_spec_name = get_asset_hub_spec_name(spec_name.as_str())
        .ok_or(GetAhmInfoError::InvalidChainSpec)?;

    // Try to find migration boundaries for the asset hub spec
    if let Some(boundaries) = get_migration_boundaries(asset_hub_spec_name) {
        return Ok((
            AhmStartEndBlocks {
                start_block: Some(boundaries.relay_migration_started_at),
                end_block: Some(boundaries.relay_migration_ended_at),
            },
            AhmStartEndBlocks {
                start_block: Some(boundaries.asset_hub_migration_started_at),
                end_block: Some(boundaries.asset_hub_migration_ended_at),
            },
        ));
    }

    // No static boundaries found, query RcMigrator pallet storage
    let rc_start = query_storage_u32(state, "RcMigrator", "MigrationStartBlock", at_hash).await;
    let rc_end = query_storage_u32(state, "RcMigrator", "MigrationEndBlock", at_hash).await;

    Ok((
        AhmStartEndBlocks {
            start_block: rc_start,
            end_block: rc_end,
        },
        AhmStartEndBlocks {
            start_block: None,
            end_block: None,
        },
    ))
}

/// Query storage for an Option<u32> value from a pallet at a specific block
/// Returns Some(value) if found, None otherwise
/// 
/// # Arguments
/// * `state` - Application state
/// * `pallet` - Pallet name (e.g., "RcMigrator")
/// * `storage_item` - Storage item name (e.g., "MigrationStartBlock")
/// * `at_hash` - Optional block hash to query at specific block. If None, queries at latest finalized block.
async fn query_storage_u32(
    state: &AppState,
    pallet: &str,
    storage_item: &str,
    at_hash: Option<&str>,
) -> Option<u32> {
    // Construct storage key using twox_128 hash
    let pallet_hash = twox_128(pallet.as_bytes());
    let storage_hash = twox_128(storage_item.as_bytes());
    
    let mut key = Vec::with_capacity(32);
    key.extend_from_slice(&pallet_hash);
    key.extend_from_slice(&storage_hash);
    
    let key_hex = format!("0x{}", hex::encode(&key));
    
    // Query storage using RPC - if at_hash is provided, query at that block
    let params = if let Some(hash) = at_hash {
        rpc_params![key_hex, hash]
    } else {
        rpc_params![key_hex]
    };
    
    match state
        .rpc_client
        .request::<Option<String>>("state_getStorage", params)
        .await
    {
        Ok(Some(value_hex)) => {
            let value_hex = value_hex.strip_prefix("0x").unwrap_or(&value_hex);
            
            if let Ok(bytes) = hex::decode(value_hex)
                && let Ok(value) = u32::decode(&mut &bytes[..])
            {
                return Some(value);
            }
            None
        }
        Ok(None) => None,
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use config::SidecarConfig;
    use crate::state::AppState;

    async fn create_state_with_url(url: &str) -> AppState {
        let mut config = SidecarConfig::default();
        config.substrate.url = url.to_string();
        AppState::new_with_config(config)
            .await
            .expect("Failed to create AppState")
    }

    #[tokio::test]
    async fn test_ahm_info_asset_hub_westmint_static_boundaries() {
        // Test: Asset Hub with static boundaries (westmint)
        // This should return static migration boundaries without querying on-chain pallets
        let state = create_state_with_url("wss://westmint-rpc.polkadot.io").await;
        
        // Verify we're connected to westmint
        assert_eq!(state.chain_info.spec_name, "westmint");
        
        let params = AhmInfoParams { at: None };
        let result = ahm_info(State(state), axum::extract::Query(params)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;
        
        assert_eq!(response.relay.start_block, Some(26041702));
        assert_eq!(response.relay.end_block, Some(26071771));
        assert_eq!(response.asset_hub.start_block, Some(11716733));
        assert_eq!(response.asset_hub.end_block, Some(11736597));
    }

    #[tokio::test]
    async fn test_ahm_info_asset_hub_westmint_with_at_block_number() {
        // Test: Asset Hub with static boundaries using `at` parameter (block number)
        let state = create_state_with_url("wss://westmint-rpc.polkadot.io").await;
        
        // Use a block number within the migration period
        let params = AhmInfoParams {
            at: Some("11720000".to_string()),
        };
        
        let result = ahm_info(State(state), axum::extract::Query(params)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;
        
        assert_eq!(response.relay.start_block, Some(26041702));
        assert_eq!(response.relay.end_block, Some(26071771));
        assert_eq!(response.asset_hub.start_block, Some(11716733));
        assert_eq!(response.asset_hub.end_block, Some(11736597));
    }

    #[tokio::test]
    async fn test_ahm_info_asset_hub_westmint_with_at_block_hash() {
        // Test: Asset Hub with static boundaries using `at` parameter (block hash)
        let state = create_state_with_url("wss://westmint-rpc.polkadot.io").await;
        
        // Get real block hash for a block within migration period
        let block_hash = state
            .get_block_hash_at_number(11720000)
            .await
            .expect("Failed to get block hash")
            .expect("Block not found");
        
        let params = AhmInfoParams {
            at: Some(block_hash),
        };
        
        let result = ahm_info(State(state), axum::extract::Query(params)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;
        
        // Should return static boundaries
        assert_eq!(response.relay.start_block, Some(26041702));
        assert_eq!(response.relay.end_block, Some(26071771));
        assert_eq!(response.asset_hub.start_block, Some(11716733));
        assert_eq!(response.asset_hub.end_block, Some(11736597));
    }

    #[tokio::test]
    async fn test_ahm_info_relay_westend_static_boundaries() {
        // Test: Relay Chain with static boundaries (westend)
        // This should map westend -> westmint and return static boundaries
        let state = create_state_with_url("wss://westend-rpc.polkadot.io").await;
        
        // Verify we're connected to westend
        assert_eq!(state.chain_info.spec_name, "westend");
        
        let params = AhmInfoParams { at: None };
        let result = ahm_info(State(state), axum::extract::Query(params)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;
        
        assert_eq!(response.relay.start_block, Some(26041702));
        assert_eq!(response.relay.end_block, Some(26071771));
        assert_eq!(response.asset_hub.start_block, Some(11716733));
        assert_eq!(response.asset_hub.end_block, Some(11736597));
    }

    #[tokio::test]
    async fn test_ahm_info_relay_westend_with_at_block_number() {
        // Test: Relay Chain with static boundaries using `at` parameter (block number)
        let state = create_state_with_url("wss://westend-rpc.polkadot.io").await;
        
        // Use a block number within the migration period
        let params = AhmInfoParams {
            at: Some("26050000".to_string()),
        };
        
        let result = ahm_info(State(state), axum::extract::Query(params)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;
        
        // Should return static boundaries
        assert_eq!(response.relay.start_block, Some(26041702));
        assert_eq!(response.relay.end_block, Some(26071771));
        assert_eq!(response.asset_hub.start_block, Some(11716733));
        assert_eq!(response.asset_hub.end_block, Some(11736597));
    }

    #[tokio::test]
    async fn test_ahm_info_relay_westend_with_at_block_hash() {
        // Test: Relay Chain with static boundaries using `at` parameter (block hash)
        let state = create_state_with_url("wss://westend-rpc.polkadot.io").await;
        
        // Get real block hash for a block within migration period
        let block_hash = state
            .get_block_hash_at_number(26050000)
            .await
            .expect("Failed to get block hash")
            .expect("Block not found");
        
        let params = AhmInfoParams {
            at: Some(block_hash),
        };
        
        let result = ahm_info(State(state), axum::extract::Query(params)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;
        
        // Should return static boundaries
        assert_eq!(response.relay.start_block, Some(26041702));
        assert_eq!(response.relay.end_block, Some(26071771));
        assert_eq!(response.asset_hub.start_block, Some(11716733));
        assert_eq!(response.asset_hub.end_block, Some(11736597));
    }

    #[tokio::test]
    async fn test_ahm_info_relay_polkadot_on_chain_pallets() {
        // Test: Relay Chain (Polkadot) with on-chain pallet querying
        // Polkadot doesn't have static boundaries, so it should query RcMigrator pallet
        let state = create_state_with_url("wss://rpc.polkadot.io").await;
        
        // Verify we're connected to polkadot
        assert_eq!(state.chain_info.spec_name, "polkadot");
        
        let params = AhmInfoParams { at: None };
        let result = ahm_info(State(state), axum::extract::Query(params)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;
        
        // Should query on-chain pallets and return real values
        // Polkadot migration start block is 28490502
        assert_eq!(response.relay.start_block, Some(28490502));
        assert_eq!(response.relay.end_block, Some(28495696));
        // Asset Hub values should be null when querying from relay chain
        assert_eq!(response.asset_hub.start_block, None);
        assert_eq!(response.asset_hub.end_block, None);
    }

    #[tokio::test]
    async fn test_ahm_info_relay_polkadot_with_at_block_number() {
        // Test: Relay Chain (Polkadot) with on-chain pallets using `at` parameter (block number)
        let state = create_state_with_url("wss://rpc.polkadot.io").await;
        
        // Use a block after migration end to ensure both start and end are available
        let params = AhmInfoParams {
            at: Some("28500000".to_string()),
        };
        
        let result = ahm_info(State(state), axum::extract::Query(params)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;
        
        // Should query on-chain pallets at the specified block
        // At a block after migration, both start and end should be available
        assert_eq!(response.relay.start_block, Some(28490502));
        assert_eq!(response.relay.end_block, Some(28495696));
        assert_eq!(response.asset_hub.start_block, None);
        assert_eq!(response.asset_hub.end_block, None);
    }

    #[tokio::test]
    async fn test_ahm_info_relay_polkadot_with_at_block_hash() {
        // Test: Relay Chain (Polkadot) with on-chain pallets using `at` parameter (block hash)
        let state = create_state_with_url("wss://rpc.polkadot.io").await;
        
        // Get real block hash for a block after migration end to ensure both values are available
        let block_hash = state
            .get_block_hash_at_number(28500000)
            .await
            .expect("Failed to get block hash")
            .expect("Block not found");
        
        let params = AhmInfoParams {
            at: Some(block_hash),
        };
        
        let result = ahm_info(State(state), axum::extract::Query(params)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;
        
        // Should query on-chain pallets at the specified block
        // At a block after migration, both start and end should be available
        assert_eq!(response.relay.start_block, Some(28490502));
        assert_eq!(response.relay.end_block, Some(28495696));
        assert_eq!(response.asset_hub.start_block, None);
        assert_eq!(response.asset_hub.end_block, None);
    }

    #[tokio::test]
    async fn test_ahm_info_relay_polkadot_with_at_before_migration() {
        // Test: Relay Chain (Polkadot) querying at a block before migration started
        let state = create_state_with_url("wss://rpc.polkadot.io").await;
        
        // Use a block before migration (28490000)
        let params = AhmInfoParams {
            at: Some("28490000".to_string()),
        };
        
        let result = ahm_info(State(state), axum::extract::Query(params)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;
        
        // Before migration, values should be null
        assert_eq!(response.relay.start_block, None);
        assert_eq!(response.relay.end_block, None);
        assert_eq!(response.asset_hub.start_block, None);
        assert_eq!(response.asset_hub.end_block, None);
    }


}
