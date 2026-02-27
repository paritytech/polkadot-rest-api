// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::extractors::JsonQuery;
use crate::handlers::pallets::common::{
    AtResponse, PalletError, format_account_id, resolve_block_for_pallet,
};
use crate::handlers::pallets::constants::is_bad_staking_block;
use crate::state::AppState;
use crate::utils::{
    BlockId, fetch_block_timestamp, find_ah_blocks_in_rc_block, resolve_block_with_rpc,
};
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use parity_scale_codec::{Compact, Decode};
use polkadot_rest_api_config::ChainType;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use subxt::{SubstrateConfig, client::OnlineClientAtBlock};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StakingValidatorsQueryParams {
    pub at: Option<String>,
    #[serde(default)]
    pub use_rc_block: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RcStakingValidatorsQueryParams {
    pub at: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StakingValidatorsResponse {
    pub at: AtResponse,
    pub validators: Vec<ValidatorInfo>,
    pub validators_to_be_chilled: Vec<ValidatorInfo>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StakingValidatorsRcResponse {
    pub at: AtResponse,
    pub validators: Vec<ValidatorInfo>,
    pub validators_to_be_chilled: Vec<ValidatorInfo>,
    pub rc_block_hash: String,
    pub rc_block_number: String,
    pub ah_timestamp: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidatorInfo {
    pub address: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commission: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked: Option<bool>,
}

#[derive(Debug, Clone, Decode)]
struct ValidatorPrefs {
    commission: Compact<u32>,
    blocked: bool,
}

#[derive(Debug, Clone, Decode)]
struct ActiveEraInfo {
    index: u32,
    #[allow(dead_code)]
    start: Option<u64>,
}

#[utoipa::path(
    get,
    path = "/v1/pallets/staking/validators",
    tag = "pallets",
    summary = "Staking validators",
    description = "Returns the list of active validators and their info.",
    params(
        ("at" = Option<String>, description = "Block hash or number to query at"),
        ("useRcBlock" = Option<bool>, description = "Treat 'at' as relay chain block identifier")
    ),
    responses(
        (status = 200, description = "Validator information", body = Object),
        (status = 503, description = "Service unavailable"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn pallets_staking_validators(
    State(state): State<AppState>,
    JsonQuery(params): JsonQuery<StakingValidatorsQueryParams>,
) -> Result<Response, PalletError> {
    if params.use_rc_block {
        return handle_use_rc_block(state, params).await;
    }

    // Resolve block using the common helper
    let resolved = resolve_block_for_pallet(&state.client, params.at.as_ref()).await?;

    let block_number = resolved.client_at_block.block_number();
    if is_bad_staking_block(&state.chain_info.spec_name, block_number) {
        return Err(PalletError::BadStakingBlock(format!(
            "Block {} is a known bad staking block for {}",
            block_number, state.chain_info.spec_name
        )));
    }

    let (validators, validators_to_be_chilled) =
        derive_staking_validators(&resolved.client_at_block, state.chain_info.ss58_prefix).await?;

    let response = StakingValidatorsResponse {
        at: resolved.at,
        validators,
        validators_to_be_chilled,
    };

    Ok((StatusCode::OK, Json(response)).into_response())
}

#[utoipa::path(
    get,
    path = "/v1/rc/pallets/staking/validators",
    tag = "rc",
    summary = "RC staking validators",
    description = "Returns the list of active validators from the relay chain.",
    params(
        ("at" = Option<String>, description = "Block hash or number to query at")
    ),
    responses(
        (status = 200, description = "Relay chain validator information", body = Object),
        (status = 503, description = "Service unavailable"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn rc_pallets_staking_validators(
    State(state): State<AppState>,
    JsonQuery(params): JsonQuery<RcStakingValidatorsQueryParams>,
) -> Result<Response, PalletError> {
    let relay_client = state.get_relay_chain_client().await?;
    let relay_rpc_client = state.get_relay_chain_rpc_client().await?;
    let relay_rpc = state.get_relay_chain_rpc().await?;
    let relay_chain_info = state.get_relay_chain_info().await?;

    let block_id = params.at.map(|s| s.parse::<BlockId>()).transpose()?;
    let resolved_block = resolve_block_with_rpc(&relay_rpc_client, &relay_rpc, block_id).await?;

    let client_at_block = relay_client.at_block(resolved_block.number).await?;

    let at = AtResponse {
        hash: format!("{:#x}", client_at_block.block_hash()),
        height: client_at_block.block_number().to_string(),
    };

    let block_number = client_at_block.block_number();
    if is_bad_staking_block(&relay_chain_info.spec_name, block_number) {
        return Err(PalletError::BadStakingBlock(format!(
            "Block {} is a known bad staking block for {}",
            block_number, relay_chain_info.spec_name
        )));
    }

    let (validators, validators_to_be_chilled) =
        derive_staking_validators(&client_at_block, relay_chain_info.ss58_prefix).await?;

    let response = StakingValidatorsResponse {
        at,
        validators,
        validators_to_be_chilled,
    };

    Ok((StatusCode::OK, Json(response)).into_response())
}

async fn handle_use_rc_block(
    state: AppState,
    params: StakingValidatorsQueryParams,
) -> Result<Response, PalletError> {
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(PalletError::UseRcBlockNotSupported);
    }

    state.get_relay_chain_client().await?;

    let rc_block_id = params
        .at
        .as_ref()
        .ok_or(PalletError::AtParameterRequired)?
        .parse::<BlockId>()?;

    let rc_rpc_client = state.get_relay_chain_rpc_client().await?;
    let rc_rpc = state.get_relay_chain_rpc().await?;

    let rc_resolved_block =
        resolve_block_with_rpc(&rc_rpc_client, &rc_rpc, Some(rc_block_id)).await?;

    let ah_blocks = find_ah_blocks_in_rc_block(&state, &rc_resolved_block).await?;

    if ah_blocks.is_empty() {
        return Ok((
            StatusCode::OK,
            Json(Vec::<StakingValidatorsRcResponse>::new()),
        )
            .into_response());
    }

    let mut results = Vec::new();

    for ah_block in &ah_blocks {
        let client_at_block = state.client.at_block(ah_block.number).await?;

        if is_bad_staking_block(&state.chain_info.spec_name, ah_block.number) {
            return Err(PalletError::BadStakingBlock(format!(
                "Block {} is a known bad staking block for {}",
                ah_block.number, state.chain_info.spec_name
            )));
        }

        let ah_timestamp = fetch_block_timestamp(&client_at_block).await;

        let (validators, validators_to_be_chilled) =
            derive_staking_validators(&client_at_block, state.chain_info.ss58_prefix).await?;

        let at = AtResponse {
            hash: ah_block.hash.clone(),
            height: ah_block.number.to_string(),
        };

        results.push(StakingValidatorsRcResponse {
            at,
            validators,
            validators_to_be_chilled,
            rc_block_hash: rc_resolved_block.hash.clone(),
            rc_block_number: rc_resolved_block.number.to_string(),
            ah_timestamp,
        });
    }

    Ok((StatusCode::OK, Json(results)).into_response())
}

/// Derives the full staking validators list with their status, commission, and blocked flag.
///
/// Returns a tuple of (all_validators, validators_to_be_chilled).
async fn derive_staking_validators(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    ss58_prefix: u16,
) -> Result<(Vec<ValidatorInfo>, Vec<ValidatorInfo>), PalletError> {
    // Get the active validator set
    let mut active_set = fetch_active_validators_set(client_at_block, ss58_prefix).await?;

    // Iterate over all Staking.Validators entries to get each validator's preferences
    let mut validators = Vec::new();

    let storage_addr =
        subxt::dynamic::storage::<([u8; 32],), scale_value::Value>("Staking", "Validators");
    let mut stream = client_at_block
        .storage()
        .iter(storage_addr, ())
        .await
        .map_err(|_| PalletError::StorageFetchFailed {
            pallet: "Staking",
            entry: "Validators",
        })?;

    while let Some(entry_result) = stream.next().await {
        let entry = match entry_result {
            Ok(e) => e,
            Err(e) => {
                return Err(PalletError::StorageEntryFetchFailed {
                    pallet: "Staking",
                    entry: "Validators",
                    error: format!("{}", e),
                });
            }
        };

        let key_bytes = entry.key_bytes();
        let value_bytes = entry.value().bytes();

        // For Twox64Concat hasher, the account_id (32 bytes) is always the last
        // 32 bytes of the key, regardless of whether the storage prefix is included.
        if key_bytes.len() < 32 {
            continue;
        }

        let mut account_id = [0u8; 32];
        account_id.copy_from_slice(&key_bytes[key_bytes.len() - 32..]);
        let address = format_account_id(&account_id, ss58_prefix);

        // Decode ValidatorPrefs: try full struct first, then just commission
        // for older runtimes that don't have the `blocked` field.
        let (commission, blocked) = if let Ok(prefs) = ValidatorPrefs::decode(&mut &value_bytes[..])
        {
            (Some(prefs.commission.0.to_string()), Some(prefs.blocked))
        } else if let Ok(commission) = Compact::<u32>::decode(&mut &value_bytes[..]) {
            (Some(commission.0.to_string()), Some(false))
        } else {
            (None, None)
        };

        let status = if active_set.remove(&address) {
            "active"
        } else {
            "waiting"
        };

        validators.push(ValidatorInfo {
            address,
            status: status.to_string(),
            commission,
            blocked,
        });
    }

    // Any validators remaining in active_set are active in the current session
    // but don't have entries in staking.validators - they're being chilled
    let mut validators_to_be_chilled = Vec::new();
    for address in &active_set {
        let info = ValidatorInfo {
            address: address.clone(),
            status: "active".to_string(),
            commission: None,
            blocked: None,
        };
        validators.push(info.clone());
        validators_to_be_chilled.push(info);
    }

    Ok((validators, validators_to_be_chilled))
}

/// Fetches the set of active validators for the current era.
///
/// First attempts to use `Staking.ErasStakersOverview` keys for the active era.
/// Falls back to `Session.Validators` if that storage entry doesn't exist.
async fn fetch_active_validators_set(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    ss58_prefix: u16,
) -> Result<HashSet<String>, PalletError> {
    // Try ErasStakersOverview first
    match fetch_active_era_index(client_at_block).await {
        Err(_) => Err(PalletError::CurrentOrActiveEraNotFound),
        Ok(active_era) => {
            if let Ok(set) =
                fetch_era_stakers_overview_keys(client_at_block, active_era, ss58_prefix).await
                && !set.is_empty()
            {
                return Ok(set);
            }

            // Fallback to Session.Validators
            return fetch_session_validators_set(client_at_block, ss58_prefix).await;
        }
    }
}

/// Fetches the active era index from Staking.ActiveEra or Staking.CurrentEra.
async fn fetch_active_era_index(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<u32, PalletError> {
    // Try ActiveEra first
    tracing::debug!("Looking for Era info using staking.activeEra");
    let storage_addr = subxt::dynamic::storage::<(), scale_value::Value>("Staking", "ActiveEra");
    if let Ok(value) = client_at_block.storage().fetch(storage_addr, ()).await {
        let bytes = value.into_bytes();
        if let Ok(era_info) = ActiveEraInfo::decode(&mut &bytes[..]) {
            return Ok(era_info.index);
        }
        // Try Option<ActiveEraInfo> wrapper
        if let Ok(Some(era_info)) = Option::<ActiveEraInfo>::decode(&mut &bytes[..]) {
            return Ok(era_info.index);
        }
    }

    // Fallback to CurrentEra
    tracing::debug!("Value for staking.activeEra not found, falling back to staking.currentEra");
    let storage_addr = subxt::dynamic::storage::<(), u32>("Staking", "CurrentEra");
    let value = client_at_block
        .storage()
        .fetch(storage_addr, ())
        .await
        .map_err(|_| PalletError::ActiveEraNotFound)?;
    value.decode().map_err(|_| PalletError::ActiveEraNotFound)
}

/// Fetches the keys of ErasStakersOverview for a given era to determine active validators.
async fn fetch_era_stakers_overview_keys(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    active_era: u32,
    ss58_prefix: u16,
) -> Result<HashSet<String>, PalletError> {
    let storage_addr = subxt::dynamic::storage::<(u32, [u8; 32]), scale_value::Value>(
        "Staking",
        "ErasStakersOverview",
    );

    let mut stream = client_at_block
        .storage()
        .iter(storage_addr, (active_era,))
        .await
        .map_err(|_| PalletError::StorageFetchFailed {
            pallet: "Staking",
            entry: "ErasStakersOverview",
        })?;

    let mut active_set = HashSet::new();

    while let Some(entry_result) = stream.next().await {
        let entry = match entry_result {
            Ok(e) => e,
            Err(e) => {
                return Err(PalletError::StorageEntryFetchFailed {
                    pallet: "Staking",
                    entry: "ErasStakersOverview",
                    error: format!("{}", e),
                });
            }
        };

        let key_bytes = entry.key_bytes();

        // For Twox64Concat hasher on the second key (AccountId32),
        // the account_id (32 bytes) is always the last 32 bytes of the key.
        if key_bytes.len() < 32 {
            continue;
        }

        let mut account_id = [0u8; 32];
        account_id.copy_from_slice(&key_bytes[key_bytes.len() - 32..]);
        active_set.insert(format_account_id(&account_id, ss58_prefix));
    }

    Ok(active_set)
}

/// Fetches Session.Validators as a fallback for the active validator set.
async fn fetch_session_validators_set(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    ss58_prefix: u16,
) -> Result<HashSet<String>, PalletError> {
    let storage_addr = subxt::dynamic::storage::<(), scale_value::Value>("Session", "Validators");
    let value = client_at_block
        .storage()
        .fetch(storage_addr, ())
        .await
        .map_err(|_| PalletError::StorageFetchFailed {
            pallet: "Session",
            entry: "Validators",
        })?;

    let bytes = value.into_bytes();
    let validators: Vec<[u8; 32]> =
        Vec::<[u8; 32]>::decode(&mut &bytes[..]).map_err(|_| PalletError::StorageDecodeFailed {
            pallet: "Session",
            entry: "Validators",
        })?;

    Ok(validators
        .iter()
        .map(|v| format_account_id(v, ss58_prefix))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_staking_validators_query_params_rejects_unknown_fields() {
        let json = r#"{"at": "12345", "unknownField": true}"#;
        let result: Result<StakingValidatorsQueryParams, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }

    #[test]
    fn test_rc_staking_validators_query_params_rejects_unknown_fields() {
        let json = r#"{"at": "12345", "unknownField": true}"#;
        let result: Result<RcStakingValidatorsQueryParams, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }
}
