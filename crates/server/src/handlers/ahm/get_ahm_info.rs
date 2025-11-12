use crate::state::AppState;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use serde_json::json;
use subxt_rpcs::rpc_params;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GetAhmInfoError {
    #[error("Failed to query storage: {0}")]
    StorageQueryFailed(String),

    #[error("RPC call failed: {0}")]
    RpcCallFailed(String),

    #[error("Failed to decode storage value: {0}")]
    DecodeFailed(String),
}

impl IntoResponse for GetAhmInfoError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            GetAhmInfoError::StorageQueryFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            GetAhmInfoError::RpcCallFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            GetAhmInfoError::DecodeFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AhmStartEndBlocks {
    pub start_block: Option<u32>,
    pub end_block: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AhmInfoResponse {
    pub relay: AhmStartEndBlocks,
    pub asset_hub: AhmStartEndBlocks,
}

/// Get Asset Hub Migration information
///
/// This endpoint returns the migration start and end blocks for both the relay chain
/// and Asset Hub. It queries the on-chain pallets:
/// - ahMigrator (Asset Hub)
/// - rcMigrator (Relay Chain)
pub async fn get_ahm_info(
    State(state): State<AppState>,
) -> Result<Json<AhmInfoResponse>, GetAhmInfoError> {
    // Query migration info from on-chain storage
    let (relay_start, relay_end, ah_start, ah_end) = query_migration_info(&state).await?;

    let response = AhmInfoResponse {
        relay: AhmStartEndBlocks {
            start_block: relay_start,
            end_block: relay_end,
        },
        asset_hub: AhmStartEndBlocks {
            start_block: ah_start,
            end_block: ah_end,
        },
    };

    Ok(Json(response))
}

/// Query migration information from the chain
///
/// Queries the following storage items:
/// - rcMigrator.migrationStartBlock
/// - rcMigrator.migrationEndBlock
/// - ahMigrator.migrationStartBlock
/// - ahMigrator.migrationEndBlock
///
/// Returns (relay_start, relay_end, ah_start, ah_end)
async fn query_migration_info(
    state: &AppState,
) -> Result<(Option<u32>, Option<u32>, Option<u32>, Option<u32>), GetAhmInfoError> {
    // Get the latest finalized block to query at
    let finalized_hash: String = state
        .legacy_rpc
        .chain_get_finalized_head()
        .await
        .map_err(|e| GetAhmInfoError::RpcCallFailed(e.to_string()))?
        .to_string();

    // Query relay chain migrator storage
    let relay_start =
        query_storage_u32(state, "RcMigrator", "MigrationStartBlock", &finalized_hash)
            .await
            .ok()
            .flatten();

    let relay_end = query_storage_u32(state, "RcMigrator", "MigrationEndBlock", &finalized_hash)
        .await
        .ok()
        .flatten();

    // Query asset hub migrator storage
    let ah_start = query_storage_u32(state, "AhMigrator", "MigrationStartBlock", &finalized_hash)
        .await
        .ok()
        .flatten();

    let ah_end = query_storage_u32(state, "AhMigrator", "MigrationEndBlock", &finalized_hash)
        .await
        .ok()
        .flatten();

    Ok((relay_start, relay_end, ah_start, ah_end))
}

/// Query a single storage item that is an Option<u32>
async fn query_storage_u32(
    state: &AppState,
    pallet: &str,
    storage_item: &str,
    at_hash: &str,
) -> Result<Option<u32>, GetAhmInfoError> {
    // Build storage key using state_getStorage RPC
    let storage_key = format!("0x{}", hex::encode(storage_key_for(pallet, storage_item)));

    // Use the rpc_client to make the state_getStorage call
    let result: Option<String> = state
        .rpc_client
        .request("state_getStorage", rpc_params![storage_key, at_hash])
        .await
        .map_err(|e| {
            GetAhmInfoError::StorageQueryFailed(format!("{}.{}: {}", pallet, storage_item, e))
        })?;

    match result {
        Some(data) => {
            // Remove 0x prefix and decode hex
            let hex_str = data.strip_prefix("0x").unwrap_or(&data);
            let bytes = hex::decode(hex_str).map_err(|e| {
                GetAhmInfoError::DecodeFailed(format!("Failed to decode hex: {}", e))
            })?;

            if bytes.is_empty() {
                return Ok(None);
            }

            // Option<u32> encoding:
            // - 0x00 = None
            // - 0x01 followed by 4 bytes (little endian u32) = Some(value)
            if bytes[0] == 0x00 {
                Ok(None)
            } else if bytes[0] == 0x01 && bytes.len() >= 5 {
                let value = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
                Ok(Some(value))
            } else {
                Err(GetAhmInfoError::DecodeFailed(format!(
                    "Invalid Option<u32> encoding for {}.{}",
                    pallet, storage_item
                )))
            }
        }
        None => Ok(None),
    }
}

/// Generate a storage key for a pallet and storage item
fn storage_key_for(pallet: &str, storage_item: &str) -> Vec<u8> {
    use sp_core::twox_128;

    let pallet_hash = twox_128(pallet.as_bytes());
    let storage_hash = twox_128(storage_item.as_bytes());

    [pallet_hash.as_ref(), storage_hash.as_ref()].concat()
}
