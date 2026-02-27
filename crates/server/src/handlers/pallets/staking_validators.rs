// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::extractors::JsonQuery;
use crate::handlers::pallets::common::{
    AtResponse, PalletError, format_account_id, resolve_block_for_pallet,
};
use crate::handlers::pallets::constants::is_bad_staking_block;
use crate::handlers::runtime_queries::staking as staking_queries;
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

    // Use centralized iteration to get all validator preferences
    let validator_entries = staking_queries::iter_staking_validators(client_at_block)
        .await
        .ok_or(PalletError::StorageFetchFailed {
            pallet: "Staking",
            entry: "Validators",
        })?;

    let mut validators = Vec::new();

    for entry in validator_entries {
        let address = format_account_id(&entry.validator, ss58_prefix);

        let status = if active_set.remove(&address) {
            "active"
        } else {
            "waiting"
        };

        validators.push(ValidatorInfo {
            address,
            status: status.to_string(),
            commission: Some(entry.commission.to_string()),
            blocked: Some(entry.blocked),
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
    if let Some(era_info) = staking_queries::get_active_era_info(client_at_block).await {
        return Ok(era_info.index);
    }

    // Fallback to CurrentEra
    tracing::debug!("Value for staking.activeEra not found, falling back to staking.currentEra");
    staking_queries::get_current_era(client_at_block)
        .await
        .ok_or(PalletError::ActiveEraNotFound)
}

/// Fetches the keys of ErasStakersOverview for a given era to determine active validators.
async fn fetch_era_stakers_overview_keys(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    active_era: u32,
    ss58_prefix: u16,
) -> Result<HashSet<String>, PalletError> {
    // Use centralized iteration function
    let validators = staking_queries::iter_era_stakers_overview_keys(client_at_block, active_era)
        .await
        .ok_or(PalletError::StorageFetchFailed {
            pallet: "Staking",
            entry: "ErasStakersOverview",
        })?;

    let active_set: HashSet<String> = validators
        .iter()
        .map(|v| format_account_id(v, ss58_prefix))
        .collect();

    Ok(active_set)
}

/// Fetches Session.Validators as a fallback for the active validator set.
async fn fetch_session_validators_set(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    ss58_prefix: u16,
) -> Result<HashSet<String>, PalletError> {
    let validators = staking_queries::get_session_validators(client_at_block, ss58_prefix)
        .await
        .ok_or(PalletError::StorageFetchFailed {
            pallet: "Session",
            entry: "Validators",
        })?;

    Ok(validators.into_iter().collect())
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
