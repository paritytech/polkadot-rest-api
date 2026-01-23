use crate::handlers::pallets::common::{
    AtResponse, PalletError, fetch_timestamp, format_account_id,
};
use crate::handlers::pallets::constants::is_bad_staking_block;
use crate::state::AppState;
use crate::utils;
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use config::ChainType;
use futures::StreamExt;
use parity_scale_codec::{Compact, Decode};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use subxt::{SubstrateConfig, client::OnlineClientAtBlock};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StakingValidatorsQueryParams {
    pub at: Option<String>,
    #[serde(default)]
    pub use_rc_block: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
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

pub async fn pallets_staking_validators(
    State(state): State<AppState>,
    Query(params): Query<StakingValidatorsQueryParams>,
) -> Result<Response, PalletError> {
    if params.use_rc_block {
        return handle_use_rc_block(state, params).await;
    }

    let client_at_block = match params.at {
        None => state.client.at_current_block().await?,
        Some(ref at_str) => {
            let block_id = at_str.parse::<utils::BlockId>()?;
            match block_id {
                utils::BlockId::Hash(hash) => state.client.at_block(hash).await?,
                utils::BlockId::Number(number) => state.client.at_block(number).await?,
            }
        }
    };

    let at = AtResponse {
        hash: format!("{:#x}", client_at_block.block_hash()),
        height: client_at_block.block_number().to_string(),
    };

    let block_number = client_at_block.block_number();
    if is_bad_staking_block(&state.chain_info.spec_name, block_number) {
        return Err(PalletError::BadStakingBlock(format!(
            "Block {} is a known bad staking block for {}",
            block_number, state.chain_info.spec_name
        )));
    }

    let (validators, validators_to_be_chilled) = derive_staking_validators(
        &client_at_block,
        state.chain_info.ss58_prefix,
        &state.chain_info.spec_name,
    )
    .await?;

    let response = StakingValidatorsResponse {
        at,
        validators,
        validators_to_be_chilled,
    };

    Ok((StatusCode::OK, Json(response)).into_response())
}

pub async fn rc_pallets_staking_validators(
    State(state): State<AppState>,
    Query(params): Query<RcStakingValidatorsQueryParams>,
) -> Result<Response, PalletError> {
    let relay_client = state
        .get_relay_chain_client()
        .ok_or(PalletError::RelayChainNotConfigured)?;
    let relay_rpc_client = state
        .get_relay_chain_rpc_client()
        .ok_or(PalletError::RelayChainNotConfigured)?;
    let relay_rpc = state
        .get_relay_chain_rpc()
        .ok_or(PalletError::RelayChainNotConfigured)?;
    let relay_chain_info = state
        .relay_chain_info
        .as_ref()
        .ok_or(PalletError::RelayChainNotConfigured)?;

    let block_id = params.at.map(|s| s.parse::<utils::BlockId>()).transpose()?;
    let resolved_block =
        utils::resolve_block_with_rpc(relay_rpc_client, relay_rpc, block_id).await?;

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

    let (validators, validators_to_be_chilled) = derive_staking_validators(
        &client_at_block,
        relay_chain_info.ss58_prefix,
        &relay_chain_info.spec_name,
    )
    .await?;

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

    if state.get_relay_chain_client().is_none() {
        return Err(PalletError::RelayChainNotConfigured);
    }

    let rc_block_id = params
        .at
        .as_ref()
        .ok_or(PalletError::AtParameterRequired)?
        .parse::<utils::BlockId>()?;

    let rc_resolved_block = utils::resolve_block_with_rpc(
        state
            .get_relay_chain_rpc_client()
            .ok_or(PalletError::RelayChainNotConfigured)?,
        state
            .get_relay_chain_rpc()
            .ok_or(PalletError::RelayChainNotConfigured)?,
        Some(rc_block_id),
    )
    .await?;

    let ah_blocks =
        crate::utils::rc_block::find_ah_blocks_in_rc_block(&state, &rc_resolved_block).await?;

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

        let ah_timestamp = fetch_timestamp(&client_at_block).await;

        let (validators, validators_to_be_chilled) = derive_staking_validators(
            &client_at_block,
            state.chain_info.ss58_prefix,
            &state.chain_info.spec_name,
        )
        .await?;

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
    spec_name: &str,
) -> Result<(Vec<ValidatorInfo>, Vec<ValidatorInfo>), PalletError> {
    // Get the active validator set
    let mut active_set =
        fetch_active_validators_set(client_at_block, ss58_prefix, spec_name).await?;

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
            Err(_) => continue,
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
    _spec_name: &str,
) -> Result<HashSet<String>, PalletError> {
    // Try ErasStakersOverview first
    if let Ok(active_era) = fetch_active_era_index(client_at_block).await
        && let Ok(set) =
            fetch_era_stakers_overview_keys(client_at_block, active_era, ss58_prefix).await
        && !set.is_empty()
    {
        return Ok(set);
    }

    // Fallback to Session.Validators
    fetch_session_validators_set(client_at_block, ss58_prefix).await
}

/// Fetches the active era index from Staking.ActiveEra or Staking.CurrentEra.
async fn fetch_active_era_index(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<u32, PalletError> {
    // Try ActiveEra first
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
            Err(_) => continue,
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
