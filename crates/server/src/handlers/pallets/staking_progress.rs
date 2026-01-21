use crate::handlers::pallets::common::{AtResponse, PalletError, format_account_id};
use crate::state::AppState;
use crate::utils;
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use config::ChainType;
use frame_metadata::RuntimeMetadata;
use hex;
use parity_scale_codec::Decode;
use serde::{Deserialize, Serialize};
use serde_json::json;
use subxt_historic::{
    SubstrateConfig,
    client::{ClientAtBlock, OnlineClientAtBlock},
};
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StakingProgressQueryParams {
    pub at: Option<String>,
    #[serde(default)]
    pub use_rc_block: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StakingProgressResponse {
    pub at: AtResponse,
    pub active_era: Option<String>,
    pub force_era: serde_json::Value,
    pub next_session_estimate: Option<String>,
    pub unapplied_slashes: Vec<UnappliedSlash>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_active_era_estimate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub election_status: Option<ElectionStatusResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ideal_validator_count: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub validator_set: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ElectionStatusResponse {
    /// When election status is deprecated, just return a string
    Deprecated(String),
    /// When election status is available, return object with status and toggle estimate
    #[serde(rename_all = "camelCase")]
    Active {
        status: serde_json::Value,
        toggle_estimate: Option<String>,
    },
}

#[derive(Debug, Serialize)]
pub struct UnappliedSlash {
    pub era: String,
    pub validator: String,
    pub own: String,
    pub others: Vec<(String, String)>,
    pub reporters: Vec<String>,
    pub payout: String,
}

#[derive(Debug, Clone, Decode)]
struct ActiveEraInfo {
    index: u32,
    #[allow(dead_code)]
    start: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Decode)]
enum Forcing {
    NotForcing,
    ForceNew,
    ForceNone,
    ForceAlways,
}

impl Forcing {
    fn to_json(&self) -> serde_json::Value {
        match self {
            Forcing::NotForcing => json!("NotForcing"),
            Forcing::ForceNew => json!("ForceNew"),
            Forcing::ForceNone => json!("ForceNone"),
            Forcing::ForceAlways => json!("ForceAlways"),
        }
    }

    fn is_force_none(&self) -> bool {
        matches!(self, Forcing::ForceNone)
    }

    fn is_force_always(&self) -> bool {
        matches!(self, Forcing::ForceAlways)
    }
}

#[derive(Debug, Clone, Decode)]
struct UnappliedSlashStorage {
    validator: [u8; 32],
    own: u128,
    others: Vec<([u8; 32], u128)>,
    reporters: Vec<[u8; 32]>,
    payout: u128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Decode)]
enum ElectionStatus {
    Close,
    Open(u32),
}

impl ElectionStatus {
    fn to_json(&self) -> serde_json::Value {
        match self {
            ElectionStatus::Close => json!({"close": null}),
            ElectionStatus::Open(block) => json!({"open": block}),
        }
    }

    fn is_close(&self) -> bool {
        matches!(self, ElectionStatus::Close)
    }
}

struct SessionEraProgress {
    era_length: u64,
    era_progress: u64,
    session_length: u64,
    session_progress: u64,
    active_era: u32,
    #[allow(dead_code)]
    current_session_index: u32,
}

struct AssetHubBabeParams {
    epoch_duration: u64,
    genesis_slot: u64,
    slot_duration_ms: u64,
}

fn get_asset_hub_babe_params(spec_name: &str) -> Option<AssetHubBabeParams> {
    match spec_name {
        "statemint" | "asset-hub-polkadot" => Some(AssetHubBabeParams {
            epoch_duration: 2400,
            genesis_slot: 265084563,
            slot_duration_ms: 6000,
        }),
        "statemine" | "asset-hub-kusama" => Some(AssetHubBabeParams {
            epoch_duration: 600,
            genesis_slot: 262493679,
            slot_duration_ms: 6000,
        }),
        "westmint" | "asset-hub-westend" => Some(AssetHubBabeParams {
            epoch_duration: 600,
            genesis_slot: 264379767,
            slot_duration_ms: 6000,
        }),
        "asset-hub-paseo" => Some(AssetHubBabeParams {
            epoch_duration: 600,
            genesis_slot: 284730328,
            slot_duration_ms: 6000,
        }),
        _ => None,
    }
}

fn is_bad_staking_block(spec_name: &str, block_number: u64) -> bool {
    match spec_name {
        // Westend Asset Hub had issues in block range 11716733 - 11746809
        "westmint" | "asset-hub-westend" => (11716733..=11746809).contains(&block_number),
        _ => false,
    }
}

pub async fn pallets_staking_progress(
    State(state): State<AppState>,
    Query(params): Query<StakingProgressQueryParams>,
) -> Result<Response, PalletError> {
    // Handle useRcBlock mode for Asset Hub
    if params.use_rc_block {
        return handle_use_rc_block(state, params).await;
    }

    // Resolve block
    let block_id = params.at.map(|s| s.parse::<utils::BlockId>()).transpose()?;
    let resolved_block = utils::resolve_block(&state, block_id).await?;
    let client_at_block = state.client.at(resolved_block.number).await?;

    let at = AtResponse {
        hash: resolved_block.hash.clone(),
        height: resolved_block.number.to_string(),
    };

    // Check for bad staking blocks
    if is_bad_staking_block(&state.chain_info.spec_name, resolved_block.number) {
        return Err(PalletError::BadStakingBlock(format!(
            "Block {} is a known bad staking block for {}",
            resolved_block.number, state.chain_info.spec_name
        )));
    }

    // Fetch base staking data
    let validator_count = fetch_validator_count(&client_at_block).await?;
    let force_era = fetch_force_era(&client_at_block).await?;

    // Fetch unapplied slashes
    let unapplied_slashes =
        fetch_unapplied_slashes(&client_at_block, state.chain_info.ss58_prefix).await;

    let is_asset_hub = state.chain_info.chain_type == ChainType::AssetHub;

    let validators = if is_asset_hub {
        if let Some(relay_client) = state.get_relay_chain_client() {
            let relay_chain_info = state
                .relay_chain_info
                .as_ref()
                .ok_or(PalletError::RelayChainNotConfigured)?;
            let relay_rpc = state
                .get_relay_chain_rpc()
                .ok_or(PalletError::RelayChainNotConfigured)?;
            let relay_block_hash = relay_rpc
                .chain_get_block_hash(None)
                .await
                .map_err(|_| PalletError::RelayChainNotConfigured)?;
            let relay_block_number = if let Some(hash) = relay_block_hash {
                let hash_str = format!("{:?}", hash);
                let header: serde_json::Value = state
                    .relay_rpc_client
                    .as_ref()
                    .ok_or(PalletError::RelayChainNotConfigured)?
                    .request("chain_getHeader", subxt_rpcs::rpc_params![hash_str])
                    .await
                    .map_err(|_| PalletError::RelayChainNotConfigured)?;
                header["number"]
                    .as_str()
                    .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
                    .unwrap_or(0)
            } else {
                0
            };
            let relay_client_at_block = relay_client.at(relay_block_number).await?;
            fetch_staking_validators(&relay_client_at_block, relay_chain_info.ss58_prefix).await?
        } else {
            fetch_staking_validators(&client_at_block, state.chain_info.ss58_prefix).await?
        }
    } else {
        fetch_staking_validators(&client_at_block, state.chain_info.ss58_prefix).await?
    };
    let progress = if is_asset_hub {
        derive_session_era_progress_asset_hub(&state, &client_at_block).await?
    } else {
        derive_session_era_progress_relay(&client_at_block, &state.chain_info.spec_name).await?
    };

    // Calculate next session estimate
    let current_block_number = resolved_block.number;
    let next_session = progress
        .session_length
        .saturating_sub(progress.session_progress)
        .saturating_add(current_block_number);

    // Build base response (always included fields)
    let mut response = StakingProgressResponse {
        at,
        active_era: Some(progress.active_era.to_string()),
        force_era: force_era.to_json(),
        next_session_estimate: Some(next_session.to_string()),
        unapplied_slashes,
        next_active_era_estimate: None,
        election_status: None,
        ideal_validator_count: None,
        validator_set: Some(validators),
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    };

    // If ForceNone (PoA network), return base response without extended fields
    if force_era.is_force_none() {
        response.validator_set = None;
        return Ok((StatusCode::OK, Json(response)).into_response());
    }

    // Calculate next active era estimate
    let next_active_era = if force_era.is_force_always() {
        next_session
    } else {
        progress
            .era_length
            .saturating_sub(progress.era_progress)
            .saturating_add(current_block_number)
    };

    // Fetch election status (may be deprecated in newer runtimes)
    let election_status = fetch_election_status(&client_at_block).await;

    // Calculate election toggle estimate
    let election_lookahead =
        derive_election_lookahead(&state.chain_info.spec_name, progress.session_length);

    let next_current_era = if next_active_era
        .saturating_sub(current_block_number)
        .saturating_sub(progress.session_length)
        > 0
    {
        next_active_era.saturating_sub(progress.session_length)
    } else {
        next_active_era
            .saturating_add(progress.era_length)
            .saturating_sub(progress.session_length)
    };

    let toggle_estimate = if election_lookahead == 0 {
        None
    } else if election_status
        .as_ref()
        .map(|s| s.is_close())
        .unwrap_or(true)
    {
        Some(next_current_era.saturating_sub(election_lookahead))
    } else {
        Some(next_current_era)
    };

    // Add extended fields
    response.next_active_era_estimate = Some(next_active_era.to_string());
    response.ideal_validator_count = Some(validator_count.to_string());
    response.election_status = Some(match election_status {
        Some(status) => ElectionStatusResponse::Active {
            status: status.to_json(),
            toggle_estimate: toggle_estimate.map(|t| t.to_string()),
        },
        None => ElectionStatusResponse::Deprecated("Deprecated, see docs".to_string()),
    });

    Ok((StatusCode::OK, Json(response)).into_response())
}

pub async fn rc_pallets_staking_progress(
    State(state): State<AppState>,
    Query(params): Query<RcStakingProgressQueryParams>,
) -> Result<Response, PalletError> {
    // Ensure relay chain is configured
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

    tracing::debug!(
        "RC staking progress: querying relay chain '{}' (spec version: {})",
        relay_chain_info.spec_name,
        relay_chain_info.spec_version
    );

    // Resolve block on relay chain
    let block_id = params.at.map(|s| s.parse::<utils::BlockId>()).transpose()?;
    let resolved_block =
        utils::resolve_block_with_rpc(relay_rpc_client, relay_rpc, block_id).await?;

    tracing::debug!(
        "RC staking progress: resolved block {} (hash: {})",
        resolved_block.number,
        resolved_block.hash
    );

    let client_at_block = relay_client.at(resolved_block.number).await?;
    tracing::debug!(
        "RC staking progress: created client at block {}",
        resolved_block.number
    );

    let at = AtResponse {
        hash: resolved_block.hash.clone(),
        height: resolved_block.number.to_string(),
    };

    // Fetch base staking data from relay chain
    let validator_count = fetch_validator_count(&client_at_block).await?;
    let force_era = fetch_force_era(&client_at_block).await?;
    let validators =
        fetch_staking_validators(&client_at_block, relay_chain_info.ss58_prefix).await?;

    // Fetch unapplied slashes
    let unapplied_slashes =
        fetch_unapplied_slashes(&client_at_block, relay_chain_info.ss58_prefix).await;

    // Derive session and era progress from relay chain (relay chains always have BABE)
    let progress =
        derive_session_era_progress_relay(&client_at_block, &relay_chain_info.spec_name).await?;

    // Calculate next session estimate
    let current_block_number = resolved_block.number;
    let next_session = progress
        .session_length
        .saturating_sub(progress.session_progress)
        .saturating_add(current_block_number);

    // Build base response
    let mut response = StakingProgressResponse {
        at,
        active_era: Some(progress.active_era.to_string()),
        force_era: force_era.to_json(),
        next_session_estimate: Some(next_session.to_string()),
        unapplied_slashes,
        next_active_era_estimate: None,
        election_status: None,
        ideal_validator_count: None,
        validator_set: Some(validators),
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    };

    // If ForceNone (PoA network), return base response
    if force_era.is_force_none() {
        response.validator_set = None;
        return Ok((StatusCode::OK, Json(response)).into_response());
    }

    // Calculate next active era estimate
    let next_active_era = if force_era.is_force_always() {
        next_session
    } else {
        progress
            .era_length
            .saturating_sub(progress.era_progress)
            .saturating_add(current_block_number)
    };

    // Fetch election status
    let election_status = fetch_election_status(&client_at_block).await;

    // Calculate election toggle estimate
    let election_lookahead =
        derive_election_lookahead(&relay_chain_info.spec_name, progress.session_length);

    let next_current_era = if next_active_era
        .saturating_sub(current_block_number)
        .saturating_sub(progress.session_length)
        > 0
    {
        next_active_era.saturating_sub(progress.session_length)
    } else {
        next_active_era
            .saturating_add(progress.era_length)
            .saturating_sub(progress.session_length)
    };

    let toggle_estimate = if election_lookahead == 0 {
        None
    } else if election_status
        .as_ref()
        .map(|s| s.is_close())
        .unwrap_or(true)
    {
        Some(next_current_era.saturating_sub(election_lookahead))
    } else {
        Some(next_current_era)
    };

    // Add extended fields
    response.next_active_era_estimate = Some(next_active_era.to_string());
    response.ideal_validator_count = Some(validator_count.to_string());
    response.election_status = Some(match election_status {
        Some(status) => ElectionStatusResponse::Active {
            status: status.to_json(),
            toggle_estimate: toggle_estimate.map(|t| t.to_string()),
        },
        None => ElectionStatusResponse::Deprecated("Deprecated, see docs".to_string()),
    });

    Ok((StatusCode::OK, Json(response)).into_response())
}

/// Query parameters for RC staking progress endpoint (no useRcBlock)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RcStakingProgressQueryParams {
    pub at: Option<String>,
}

async fn handle_use_rc_block(
    state: AppState,
    params: StakingProgressQueryParams,
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
            .expect("relay chain client checked above"),
        state
            .get_relay_chain_rpc()
            .expect("relay chain rpc checked above"),
        Some(rc_block_id),
    )
    .await?;

    let ah_blocks =
        crate::utils::rc_block::find_ah_blocks_in_rc_block(&state, &rc_resolved_block).await?;

    if ah_blocks.is_empty() {
        return Ok(build_empty_rc_response(&rc_resolved_block));
    }

    let ah_block = &ah_blocks[0];
    let client_at_block = state.client.at(ah_block.number).await?;

    let at = AtResponse {
        hash: ah_block.hash.clone(),
        height: ah_block.number.to_string(),
    };

    // Check for bad staking blocks
    if is_bad_staking_block(&state.chain_info.spec_name, ah_block.number) {
        return Err(PalletError::BadStakingBlock(format!(
            "Block {} is a known bad staking block for {}",
            ah_block.number, state.chain_info.spec_name
        )));
    }

    // Fetch timestamp for Asset Hub
    let ah_timestamp = fetch_timestamp(&client_at_block).await;

    // Fetch base staking data
    let validator_count = fetch_validator_count(&client_at_block).await?;
    let force_era = fetch_force_era(&client_at_block).await?;
    let validators =
        fetch_staking_validators(&client_at_block, state.chain_info.ss58_prefix).await?;

    // Fetch unapplied slashes
    let unapplied_slashes =
        fetch_unapplied_slashes(&client_at_block, state.chain_info.ss58_prefix).await;

    // Derive session and era progress for Asset Hub
    let progress = derive_session_era_progress_asset_hub(&state, &client_at_block).await?;

    // Calculate estimates
    let current_block_number = ah_block.number;
    let next_session = progress
        .session_length
        .saturating_sub(progress.session_progress)
        .saturating_add(current_block_number);

    let mut response = StakingProgressResponse {
        at,
        active_era: Some(progress.active_era.to_string()),
        force_era: force_era.to_json(),
        next_session_estimate: Some(next_session.to_string()),
        unapplied_slashes,
        next_active_era_estimate: None,
        election_status: None,
        ideal_validator_count: None,
        validator_set: Some(validators),
        rc_block_hash: Some(rc_resolved_block.hash),
        rc_block_number: Some(rc_resolved_block.number.to_string()),
        ah_timestamp,
    };

    if force_era.is_force_none() {
        response.validator_set = None;
        return Ok((StatusCode::OK, Json(response)).into_response());
    }

    let next_active_era = if force_era.is_force_always() {
        next_session
    } else {
        progress
            .era_length
            .saturating_sub(progress.era_progress)
            .saturating_add(current_block_number)
    };

    let election_status = fetch_election_status(&client_at_block).await;
    let election_lookahead =
        derive_election_lookahead(&state.chain_info.spec_name, progress.session_length);

    let next_current_era = if next_active_era
        .saturating_sub(current_block_number)
        .saturating_sub(progress.session_length)
        > 0
    {
        next_active_era.saturating_sub(progress.session_length)
    } else {
        next_active_era
            .saturating_add(progress.era_length)
            .saturating_sub(progress.session_length)
    };

    let toggle_estimate = if election_lookahead == 0 {
        None
    } else if election_status
        .as_ref()
        .map(|s| s.is_close())
        .unwrap_or(true)
    {
        Some(next_current_era.saturating_sub(election_lookahead))
    } else {
        Some(next_current_era)
    };

    response.next_active_era_estimate = Some(next_active_era.to_string());
    response.ideal_validator_count = Some(validator_count.to_string());
    response.election_status = Some(match election_status {
        Some(status) => ElectionStatusResponse::Active {
            status: status.to_json(),
            toggle_estimate: toggle_estimate.map(|t| t.to_string()),
        },
        None => ElectionStatusResponse::Deprecated("Deprecated, see docs".to_string()),
    });

    Ok((StatusCode::OK, Json(response)).into_response())
}

fn build_empty_rc_response(rc_resolved_block: &utils::ResolvedBlock) -> Response {
    let at = AtResponse {
        hash: rc_resolved_block.hash.clone(),
        height: rc_resolved_block.number.to_string(),
    };

    (
        StatusCode::OK,
        Json(StakingProgressResponse {
            at,
            active_era: None,
            force_era: json!(null),
            next_session_estimate: None,
            unapplied_slashes: vec![],
            next_active_era_estimate: None,
            election_status: None,
            ideal_validator_count: None,
            validator_set: None,
            rc_block_hash: Some(rc_resolved_block.hash.clone()),
            rc_block_number: Some(rc_resolved_block.number.to_string()),
            ah_timestamp: None,
        }),
    )
        .into_response()
}

async fn fetch_validator_count<'client>(
    client_at_block: &ClientAtBlock<OnlineClientAtBlock<'client, SubstrateConfig>, SubstrateConfig>,
) -> Result<u32, PalletError> {
    let storage = client_at_block
        .storage()
        .entry("Staking", "ValidatorCount")
        .map_err(|_| PalletError::StakingPalletNotFound)?;

    let value = storage
        .fetch(())
        .await
        .map_err(|_| PalletError::StakingPalletNotFound)?
        .ok_or(PalletError::StakingPalletNotFound)?;

    let bytes = value.into_bytes();
    u32::decode(&mut &bytes[..]).map_err(|_| PalletError::StakingPalletNotFound)
}

async fn fetch_force_era<'client>(
    client_at_block: &ClientAtBlock<OnlineClientAtBlock<'client, SubstrateConfig>, SubstrateConfig>,
) -> Result<Forcing, PalletError> {
    let storage = client_at_block
        .storage()
        .entry("Staking", "ForceEra")
        .map_err(|_| PalletError::StakingPalletNotFound)?;

    let value = storage
        .fetch(())
        .await
        .map_err(|_| PalletError::StakingPalletNotFound)?
        .ok_or(PalletError::StakingPalletNotFound)?;

    let bytes = value.into_bytes();
    Forcing::decode(&mut &bytes[..]).map_err(|_| PalletError::StakingPalletNotFound)
}

async fn fetch_active_era<'client>(
    client_at_block: &ClientAtBlock<OnlineClientAtBlock<'client, SubstrateConfig>, SubstrateConfig>,
) -> Result<ActiveEraInfo, PalletError> {
    let storage = client_at_block
        .storage()
        .entry("Staking", "ActiveEra")
        .map_err(|e| {
            tracing::error!("Failed to get Staking.ActiveEra storage entry: {:?}", e);
            PalletError::StakingPalletNotFound
        })?;

    let value = storage
        .fetch(())
        .await
        .map_err(|e| {
            tracing::error!("Failed to fetch Staking.ActiveEra: {:?}", e);
            PalletError::StakingPalletNotFound
        })?
        .ok_or_else(|| {
            tracing::error!("Staking.ActiveEra storage entry does not exist at this block");
            PalletError::ActiveEraNotFound
        })?;

    let bytes = value.into_bytes();
    tracing::debug!("Staking.ActiveEra raw bytes: {:?}", hex::encode(&bytes));

    if let Ok(era_info) = ActiveEraInfo::decode(&mut &bytes[..]) {
        tracing::debug!("Decoded ActiveEraInfo directly: index={}", era_info.index);
        return Ok(era_info);
    }

    let option_value: Option<ActiveEraInfo> = Option::<ActiveEraInfo>::decode(&mut &bytes[..])
        .map_err(|e| {
            tracing::error!("Failed to decode Staking.ActiveEra: {:?}", e);
            PalletError::ActiveEraNotFound
        })?;

    option_value.ok_or_else(|| {
        tracing::error!("Staking.ActiveEra is None (no active era at this block)");
        PalletError::ActiveEraNotFound
    })
}

async fn fetch_bonded_eras<'client>(
    client_at_block: &ClientAtBlock<OnlineClientAtBlock<'client, SubstrateConfig>, SubstrateConfig>,
) -> Result<Vec<(u32, u32)>, PalletError> {
    let storage = client_at_block
        .storage()
        .entry("Staking", "BondedEras")
        .map_err(|_| PalletError::StakingPalletNotFound)?;

    let value = storage
        .fetch(())
        .await
        .map_err(|_| PalletError::StakingPalletNotFound)?
        .ok_or(PalletError::EraStartSessionNotFound)?;

    let bytes = value.into_bytes();
    Vec::<(u32, u32)>::decode(&mut &bytes[..]).map_err(|_| PalletError::EraStartSessionNotFound)
}

async fn fetch_staking_validators<'client>(
    client_at_block: &ClientAtBlock<OnlineClientAtBlock<'client, SubstrateConfig>, SubstrateConfig>,
    ss58_prefix: u16,
) -> Result<Vec<String>, PalletError> {
    if let Ok(storage) = client_at_block.storage().entry("Staking", "Validators")
        && let Ok(Some(value)) = storage.fetch(()).await
    {
        let bytes = value.into_bytes();
        if let Ok(validators) = Vec::<[u8; 32]>::decode(&mut &bytes[..]) {
            tracing::debug!(
                "Found {} validators in Staking.Validators",
                validators.len()
            );
            return Ok(validators
                .iter()
                .map(|v| format_account_id(v, ss58_prefix))
                .collect());
        }
    }

    let storage = client_at_block
        .storage()
        .entry("Session", "Validators")
        .map_err(|e| {
            tracing::debug!("Session.Validators not found: {:?}", e);
            PalletError::SessionPalletNotFound
        })?;

    let value = storage
        .fetch(())
        .await
        .map_err(|e| {
            tracing::debug!("Failed to fetch Session.Validators: {:?}", e);
            PalletError::SessionPalletNotFound
        })?
        .ok_or_else(|| {
            tracing::debug!("Session.Validators storage is empty");
            PalletError::SessionPalletNotFound
        })?;

    let bytes = value.into_bytes();
    let validators: Vec<[u8; 32]> = Vec::<[u8; 32]>::decode(&mut &bytes[..]).map_err(|e| {
        tracing::debug!("Failed to decode Session.Validators: {:?}", e);
        PalletError::SessionPalletNotFound
    })?;

    Ok(validators
        .iter()
        .map(|v| format_account_id(v, ss58_prefix))
        .collect())
}

async fn fetch_unapplied_slashes<'client>(
    client_at_block: &ClientAtBlock<OnlineClientAtBlock<'client, SubstrateConfig>, SubstrateConfig>,
    ss58_prefix: u16,
) -> Vec<UnappliedSlash> {
    use futures::StreamExt;

    let storage = match client_at_block
        .storage()
        .entry("Staking", "UnappliedSlashes")
    {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    let mut stream = match storage.iter(()).await {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    let mut result = Vec::new();

    while let Some(entry_result) = stream.next().await {
        let entry = match entry_result {
            Ok(e) => e,
            Err(_) => continue,
        };

        let (key_bytes, value_bytes) = entry.into_key_and_value_bytes();

        // Key format: 16 bytes pallet prefix + 16 bytes entry prefix + 8 bytes twox64 hash + 4 bytes era
        // Total: 44 bytes, era starts at byte 40
        let era: u32 = if key_bytes.len() >= 44 {
            u32::decode(&mut &key_bytes[40..44]).unwrap_or(0)
        } else {
            continue;
        };
        let slashes: Vec<UnappliedSlashStorage> =
            match Vec::<UnappliedSlashStorage>::decode(&mut &value_bytes[..]) {
                Ok(s) => s,
                Err(_) => continue,
            };

        for slash in slashes {
            result.push(UnappliedSlash {
                era: era.to_string(),
                validator: format_account_id(&slash.validator, ss58_prefix),
                own: slash.own.to_string(),
                others: slash
                    .others
                    .iter()
                    .map(|(acc, amount)| (format_account_id(acc, ss58_prefix), amount.to_string()))
                    .collect(),
                reporters: slash
                    .reporters
                    .iter()
                    .map(|acc| format_account_id(acc, ss58_prefix))
                    .collect(),
                payout: slash.payout.to_string(),
            });
        }
    }

    result
}

async fn fetch_election_status<'client>(
    client_at_block: &ClientAtBlock<OnlineClientAtBlock<'client, SubstrateConfig>, SubstrateConfig>,
) -> Option<ElectionStatus> {
    let storage = client_at_block
        .storage()
        .entry("Staking", "EraElectionStatus")
        .ok()?;

    let value = storage.fetch(()).await.ok()??;
    let bytes = value.into_bytes();
    ElectionStatus::decode(&mut &bytes[..]).ok()
}

async fn fetch_timestamp<'client>(
    client_at_block: &ClientAtBlock<OnlineClientAtBlock<'client, SubstrateConfig>, SubstrateConfig>,
) -> Option<String> {
    let storage = client_at_block.storage().entry("Timestamp", "Now").ok()?;
    let value = storage.fetch(()).await.ok()??;
    let bytes = value.into_bytes();
    let timestamp = u64::decode(&mut &bytes[..]).ok()?;
    Some(timestamp.to_string())
}

// ============================================================================
// Session/Era Progress Derivation
// ============================================================================

async fn derive_session_era_progress_relay<'client>(
    client_at_block: &ClientAtBlock<OnlineClientAtBlock<'client, SubstrateConfig>, SubstrateConfig>,
    spec_name: &str,
) -> Result<SessionEraProgress, PalletError> {
    // Fetch BABE storage items
    let current_slot = fetch_babe_current_slot(client_at_block).await?;
    let epoch_index = fetch_babe_epoch_index(client_at_block).await?;
    let genesis_slot = fetch_babe_genesis_slot(client_at_block).await?;

    // Fetch session current index
    let current_index = fetch_session_current_index(client_at_block).await?;

    // Fetch active era and bonded eras
    let active_era_info = fetch_active_era(client_at_block).await?;
    let bonded_eras = fetch_bonded_eras(client_at_block).await?;

    // Get chain constants from metadata
    let sessions_per_era = get_sessions_per_era_from_metadata(client_at_block.metadata())
        .ok_or(PalletError::StakingPalletNotFound)?;
    let epoch_duration = get_babe_epoch_duration(spec_name);

    // Find active era start session index
    let active_era_start_session = bonded_eras
        .iter()
        .find(|(era, _)| *era == active_era_info.index)
        .map(|(_, session)| *session)
        .ok_or(PalletError::EraStartSessionNotFound)?;

    let session_length = epoch_duration;
    let era_length = (sessions_per_era as u64) * session_length;
    let epoch_start_slot = epoch_index * session_length + genesis_slot;
    let session_progress = current_slot.saturating_sub(epoch_start_slot);
    let era_progress =
        ((current_index - active_era_start_session) as u64) * session_length + session_progress;

    Ok(SessionEraProgress {
        era_length,
        era_progress,
        session_length,
        session_progress,
        active_era: active_era_info.index,
        current_session_index: current_index,
    })
}

async fn derive_session_era_progress_asset_hub<'client>(
    state: &AppState,
    client_at_block: &ClientAtBlock<OnlineClientAtBlock<'client, SubstrateConfig>, SubstrateConfig>,
) -> Result<SessionEraProgress, PalletError> {
    let babe_params = get_asset_hub_babe_params(&state.chain_info.spec_name)
        .ok_or(PalletError::StakingPalletNotFound)?;

    // Fetch timestamp
    let timestamp_str = fetch_timestamp(client_at_block)
        .await
        .ok_or(PalletError::StakingPalletNotFound)?;
    let timestamp: u64 = timestamp_str
        .parse()
        .map_err(|_| PalletError::StakingPalletNotFound)?;

    // Fetch active era and bonded eras
    let active_era_info = fetch_active_era(client_at_block).await?;
    let bonded_eras = fetch_bonded_eras(client_at_block).await?;

    // Get sessions per era from metadata
    let sessions_per_era = get_sessions_per_era_from_metadata(client_at_block.metadata())
        .ok_or(PalletError::StakingPalletNotFound)?;

    // Find active era start session index
    let active_era_start_session = bonded_eras
        .iter()
        .find(|(era, _)| *era == active_era_info.index)
        .map(|(_, session)| *session)
        .ok_or(PalletError::EraStartSessionNotFound)?;

    // Calculate current slot from timestamp
    let current_slot = timestamp / babe_params.slot_duration_ms;

    // Calculate epoch index
    let epoch_index =
        (current_slot.saturating_sub(babe_params.genesis_slot)) / babe_params.epoch_duration;

    // Assume session = epoch (skipped epochs handling can be enhanced later)
    let current_index = epoch_index as u32;

    // Calculate progress
    let epoch_start_slot = epoch_index * babe_params.epoch_duration + babe_params.genesis_slot;
    let session_progress = current_slot.saturating_sub(epoch_start_slot);
    let era_length = (sessions_per_era as u64) * babe_params.epoch_duration;
    let era_progress = ((current_index - active_era_start_session) as u64)
        * babe_params.epoch_duration
        + session_progress;

    Ok(SessionEraProgress {
        era_length,
        era_progress,
        session_length: babe_params.epoch_duration,
        session_progress,
        active_era: active_era_info.index,
        current_session_index: current_index,
    })
}

// Reserved for future implementation when relay chain block resolution is added
#[allow(dead_code)]
async fn fetch_relay_skipped_epochs<'client>(
    client_at_block: &ClientAtBlock<OnlineClientAtBlock<'client, SubstrateConfig>, SubstrateConfig>,
) -> Vec<(u64, u32)> {
    let storage = match client_at_block.storage().entry("Babe", "SkippedEpochs") {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    let value = match storage.fetch(()).await {
        Ok(Some(v)) => v,
        _ => return vec![],
    };

    let bytes = value.into_bytes();
    Vec::<(u64, u32)>::decode(&mut &bytes[..]).unwrap_or_default()
}

#[allow(dead_code)]
fn calculate_session_from_skipped_epochs(epoch_index: u64, skipped_epochs: &[(u64, u32)]) -> u32 {
    if skipped_epochs.is_empty() {
        return epoch_index as u32;
    }

    // Sort by epoch index
    let mut sorted: Vec<_> = skipped_epochs.to_vec();
    sorted.sort_by_key(|(epoch, _)| *epoch);

    // Find closest skipped epoch <= current epoch
    let closest = sorted
        .iter()
        .filter(|(epoch, _)| *epoch <= epoch_index)
        .next_back();

    match closest {
        Some((skipped_epoch, skipped_session)) => {
            let permanent_offset = skipped_epoch - (*skipped_session as u64);
            (epoch_index - permanent_offset) as u32
        }
        None => epoch_index as u32,
    }
}

async fn fetch_babe_current_slot<'client>(
    client_at_block: &ClientAtBlock<OnlineClientAtBlock<'client, SubstrateConfig>, SubstrateConfig>,
) -> Result<u64, PalletError> {
    let storage = client_at_block
        .storage()
        .entry("Babe", "CurrentSlot")
        .map_err(|_| PalletError::PalletNotFound("Babe".to_string()))?;

    let value = storage
        .fetch(())
        .await
        .map_err(|_| PalletError::PalletNotFound("Babe".to_string()))?
        .ok_or(PalletError::PalletNotFound("Babe".to_string()))?;

    let bytes = value.into_bytes();
    u64::decode(&mut &bytes[..]).map_err(|_| PalletError::PalletNotFound("Babe".to_string()))
}

async fn fetch_babe_epoch_index<'client>(
    client_at_block: &ClientAtBlock<OnlineClientAtBlock<'client, SubstrateConfig>, SubstrateConfig>,
) -> Result<u64, PalletError> {
    let storage = client_at_block
        .storage()
        .entry("Babe", "EpochIndex")
        .map_err(|_| PalletError::PalletNotFound("Babe".to_string()))?;

    let value = storage
        .fetch(())
        .await
        .map_err(|_| PalletError::PalletNotFound("Babe".to_string()))?
        .ok_or(PalletError::PalletNotFound("Babe".to_string()))?;

    let bytes = value.into_bytes();
    u64::decode(&mut &bytes[..]).map_err(|_| PalletError::PalletNotFound("Babe".to_string()))
}

async fn fetch_babe_genesis_slot<'client>(
    client_at_block: &ClientAtBlock<OnlineClientAtBlock<'client, SubstrateConfig>, SubstrateConfig>,
) -> Result<u64, PalletError> {
    let storage = client_at_block
        .storage()
        .entry("Babe", "GenesisSlot")
        .map_err(|_| PalletError::PalletNotFound("Babe".to_string()))?;

    let value = storage
        .fetch(())
        .await
        .map_err(|_| PalletError::PalletNotFound("Babe".to_string()))?
        .ok_or(PalletError::PalletNotFound("Babe".to_string()))?;

    let bytes = value.into_bytes();
    u64::decode(&mut &bytes[..]).map_err(|_| PalletError::PalletNotFound("Babe".to_string()))
}

async fn fetch_session_current_index<'client>(
    client_at_block: &ClientAtBlock<OnlineClientAtBlock<'client, SubstrateConfig>, SubstrateConfig>,
) -> Result<u32, PalletError> {
    let storage = client_at_block
        .storage()
        .entry("Session", "CurrentIndex")
        .map_err(|_| PalletError::SessionPalletNotFound)?;

    let value = storage
        .fetch(())
        .await
        .map_err(|_| PalletError::SessionPalletNotFound)?
        .ok_or(PalletError::SessionPalletNotFound)?;

    let bytes = value.into_bytes();
    u32::decode(&mut &bytes[..]).map_err(|_| PalletError::SessionPalletNotFound)
}

fn get_sessions_per_era_from_metadata(metadata: &RuntimeMetadata) -> Option<u32> {
    match metadata {
        RuntimeMetadata::V14(m) => {
            for pallet in &m.pallets {
                if pallet.name == "Staking" {
                    for constant in &pallet.constants {
                        if constant.name == "SessionsPerEra" {
                            return u32::decode(&mut &constant.value[..]).ok();
                        }
                    }
                }
            }
            None
        }
        RuntimeMetadata::V15(m) => {
            for pallet in &m.pallets {
                if pallet.name == "Staking" {
                    for constant in &pallet.constants {
                        if constant.name == "SessionsPerEra" {
                            return u32::decode(&mut &constant.value[..]).ok();
                        }
                    }
                }
            }
            None
        }
        RuntimeMetadata::V16(m) => {
            for pallet in &m.pallets {
                if pallet.name == "Staking" {
                    for constant in &pallet.constants {
                        if constant.name == "SessionsPerEra" {
                            return u32::decode(&mut &constant.value[..]).ok();
                        }
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn get_babe_epoch_duration(spec_name: &str) -> u64 {
    match spec_name {
        "polkadot" => 2400,
        _ => 600,
    }
}

fn derive_election_lookahead(spec_name: &str, epoch_duration: u64) -> u64 {
    let divisor = match spec_name {
        "polkadot" | "statemint" | "asset-hub-polkadot" => 16,
        _ => 4,
    };
    epoch_duration / divisor
}
