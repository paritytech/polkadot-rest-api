// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::extractors::JsonQuery;
use crate::handlers::pallets::common::{
    AtResponse, PalletError, format_account_id, resolve_block_for_pallet,
};
use crate::handlers::pallets::constants::{
    derive_election_lookahead, get_asset_hub_babe_params, get_babe_epoch_duration,
    is_bad_staking_block,
};
use crate::handlers::runtime_queries::staking as staking_queries;
use crate::state::{AppState, RelayChainError};
use crate::utils::{
    BlockId, DEFAULT_CONCURRENCY, fetch_block_timestamp, rc_block::find_ah_blocks_in_rc_block,
    resolve_block_with_rpc, run_with_concurrency_collect,
};
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use parity_scale_codec::Decode;
use polkadot_rest_api_config::ChainType;
use serde::{Deserialize, Serialize};
use serde_json::json;
use subxt::{SubstrateConfig, client::OnlineClientAtBlock};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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

impl From<staking_queries::DecodedActiveEraInfo> for ActiveEraInfo {
    fn from(info: staking_queries::DecodedActiveEraInfo) -> Self {
        ActiveEraInfo {
            index: info.index,
            start: info.start,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Decode)]
enum ForceEra {
    NotForcing,
    ForceNew,
    ForceNone,
    ForceAlways,
}

impl From<staking_queries::ForceEra> for ForceEra {
    fn from(fe: staking_queries::ForceEra) -> Self {
        match fe {
            staking_queries::ForceEra::NotForcing => ForceEra::NotForcing,
            staking_queries::ForceEra::ForceNew => ForceEra::ForceNew,
            staking_queries::ForceEra::ForceNone => ForceEra::ForceNone,
            staking_queries::ForceEra::ForceAlways => ForceEra::ForceAlways,
        }
    }
}

impl ForceEra {
    fn to_json(self) -> serde_json::Value {
        match self {
            ForceEra::NotForcing => json!("NotForcing"),
            ForceEra::ForceNew => json!("ForceNew"),
            ForceEra::ForceNone => json!("ForceNone"),
            ForceEra::ForceAlways => json!("ForceAlways"),
        }
    }

    fn is_force_none(&self) -> bool {
        matches!(self, ForceEra::ForceNone)
    }

    fn is_force_always(&self) -> bool {
        matches!(self, ForceEra::ForceAlways)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Decode)]
enum ElectionStatus {
    Close,
    Open(u32),
}

impl From<staking_queries::EraElectionStatus> for ElectionStatus {
    fn from(status: staking_queries::EraElectionStatus) -> Self {
        match status {
            staking_queries::EraElectionStatus::Close => ElectionStatus::Close,
            staking_queries::EraElectionStatus::Open(block) => ElectionStatus::Open(block),
        }
    }
}

impl ElectionStatus {
    fn to_json(self) -> serde_json::Value {
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

#[utoipa::path(
    get,
    path = "/v1/pallets/staking/progress",
    tag = "pallets",
    summary = "Staking progress",
    description = "Returns staking progress including era, session info, and validator counts.",
    params(
        ("at" = Option<String>, description = "Block hash or number to query at"),
        ("useRcBlock" = Option<bool>, description = "Treat 'at' as relay chain block identifier")
    ),
    responses(
        (status = 200, description = "Staking progress information", body = Object),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn pallets_staking_progress(
    State(state): State<AppState>,
    JsonQuery(params): JsonQuery<StakingProgressQueryParams>,
) -> Result<Response, PalletError> {
    // Handle useRcBlock mode for Asset Hub
    if params.use_rc_block {
        return handle_use_rc_block(state, params).await;
    }

    // Resolve block using the common helper
    let resolved = resolve_block_for_pallet(&state.client, params.at.as_ref()).await?;

    // Check for bad staking blocks
    let block_number = resolved.client_at_block.block_number();
    if is_bad_staking_block(&state.chain_info.spec_name, block_number) {
        return Err(PalletError::BadStakingBlock(format!(
            "Block {} is a known bad staking block for {}",
            block_number, state.chain_info.spec_name
        )));
    }

    // Fetch base staking data
    let validator_count = fetch_validator_count(&resolved.client_at_block).await?;
    let force_era = fetch_force_era(&resolved.client_at_block).await?;

    // Fetch unapplied slashes
    let unapplied_slashes =
        fetch_unapplied_slashes(&resolved.client_at_block, state.chain_info.ss58_prefix).await;

    let is_asset_hub = state.chain_info.chain_type == ChainType::AssetHub;

    let validators = if is_asset_hub {
        if let Ok(relay_client) = state.get_relay_chain_client().await {
            let relay_chain_info = state.get_relay_chain_info().await?;
            let relay_rpc = state.get_relay_chain_rpc().await?;
            let relay_block_hash = relay_rpc
                .chain_get_block_hash(None)
                .await
                .map_err(|e| RelayChainError::ConnectionFailed(e.to_string()))?;
            let relay_rpc_client = state.get_relay_chain_rpc_client().await?;
            let relay_block_number = if let Some(hash) = relay_block_hash {
                let hash_str = format!("{:?}", hash);
                let header: serde_json::Value = relay_rpc_client
                    .request("chain_getHeader", subxt_rpcs::rpc_params![hash_str])
                    .await
                    .map_err(|e| PalletError::StorageEntryFetchFailed {
                        pallet: "System",
                        entry: "BlockHash",
                        error: e.to_string(),
                    })?;
                header["number"]
                    .as_str()
                    .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
                    .unwrap_or(0)
            } else {
                0
            };
            let relay_client_at_block = relay_client.at_block(relay_block_number).await?;
            fetch_staking_validators(&relay_client_at_block, relay_chain_info.ss58_prefix).await?
        } else {
            fetch_staking_validators(&resolved.client_at_block, state.chain_info.ss58_prefix)
                .await?
        }
    } else {
        fetch_staking_validators(&resolved.client_at_block, state.chain_info.ss58_prefix).await?
    };
    let progress = if is_asset_hub {
        derive_session_era_progress_asset_hub(&state, &resolved.client_at_block).await?
    } else {
        derive_session_era_progress_relay(&resolved.client_at_block, &state.chain_info.spec_name)
            .await?
    };

    // Calculate next session estimate
    let current_block_number = block_number;
    let next_session = progress
        .session_length
        .saturating_sub(progress.session_progress)
        .saturating_add(current_block_number);

    // Build base response (always included fields)
    let mut response = StakingProgressResponse {
        at: resolved.at,
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
    let election_status = fetch_election_status(&resolved.client_at_block).await;

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

#[utoipa::path(
    get,
    path = "/v1/rc/pallets/staking/progress",
    tag = "rc",
    summary = "RC staking progress",
    description = "Returns staking progress from the relay chain.",
    params(
        ("at" = Option<String>, description = "Block hash or number to query at")
    ),
    responses(
        (status = 200, description = "Relay chain staking progress", body = Object),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn rc_pallets_staking_progress(
    State(state): State<AppState>,
    JsonQuery(params): JsonQuery<RcStakingProgressQueryParams>,
) -> Result<Response, PalletError> {
    // Ensure relay chain is configured
    let relay_client = state.get_relay_chain_client().await?;
    let relay_rpc_client = state.get_relay_chain_rpc_client().await?;
    let relay_rpc = state.get_relay_chain_rpc().await?;
    let relay_chain_info = state.get_relay_chain_info().await?;

    tracing::debug!(
        "RC staking progress: querying relay chain '{}' (spec version: {})",
        relay_chain_info.spec_name,
        relay_chain_info.spec_version
    );

    // Resolve block on relay chain
    let block_id = params.at.map(|s| s.parse::<BlockId>()).transpose()?;
    let resolved_block = resolve_block_with_rpc(&relay_rpc_client, &relay_rpc, block_id).await?;

    tracing::debug!(
        "RC staking progress: resolved block {} (hash: {})",
        resolved_block.number,
        resolved_block.hash
    );

    let client_at_block = relay_client.at_block(resolved_block.number).await?;
    tracing::debug!(
        "RC staking progress: created client at block {}",
        client_at_block.block_number()
    );

    let at = AtResponse {
        hash: format!("{:#x}", client_at_block.block_hash()),
        height: client_at_block.block_number().to_string(),
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
    let current_block_number = client_at_block.block_number();
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
        return Ok((StatusCode::OK, Json(Vec::<StakingProgressResponse>::new())).into_response());
    }

    let rc_hash = rc_resolved_block.hash.clone();
    let rc_number = rc_resolved_block.number.to_string();
    let spec_name = state.chain_info.spec_name.clone();
    let ss58_prefix = state.chain_info.ss58_prefix;

    let futures =
        ah_blocks.iter().map(|ah_block| {
            let state = state.clone();
            let rc_hash = rc_hash.clone();
            let rc_number = rc_number.clone();
            let spec_name = spec_name.clone();
            let ah_block_hash = ah_block.hash.clone();
            let ah_block_number = ah_block.number;

            async move {
                let client_at_block = state.client.at_block(ah_block_number).await?;

                let at = AtResponse {
                    hash: ah_block_hash,
                    height: ah_block_number.to_string(),
                };

                // Check for bad staking blocks
                if is_bad_staking_block(&spec_name, ah_block_number) {
                    return Err(PalletError::BadStakingBlock(format!(
                        "Block {} is a known bad staking block for {}",
                        ah_block_number, spec_name
                    )));
                }

                // Fetch data in parallel where possible
                let (
                    ah_timestamp,
                    validator_count,
                    force_era,
                    validators,
                    unapplied_slashes,
                    progress,
                ) = tokio::try_join!(
                    async { Ok::<_, PalletError>(fetch_block_timestamp(&client_at_block).await) },
                    fetch_validator_count(&client_at_block),
                    fetch_force_era(&client_at_block),
                    fetch_staking_validators(&client_at_block, ss58_prefix),
                    async {
                        Ok::<_, PalletError>(
                            fetch_unapplied_slashes(&client_at_block, ss58_prefix).await,
                        )
                    },
                    derive_session_era_progress_asset_hub(&state, &client_at_block)
                )?;

                // Calculate estimates
                let current_block_number = ah_block_number;
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
                    rc_block_hash: Some(rc_hash),
                    rc_block_number: Some(rc_number),
                    ah_timestamp,
                };

                if force_era.is_force_none() {
                    response.validator_set = None;
                    return Ok(response);
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
                    derive_election_lookahead(&spec_name, progress.session_length);

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

                Ok(response)
            }
        });

    let responses = run_with_concurrency_collect(DEFAULT_CONCURRENCY, futures).await?;

    Ok((StatusCode::OK, Json(responses)).into_response())
}

async fn fetch_validator_count(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<u32, PalletError> {
    staking_queries::get_validator_count(client_at_block)
        .await
        .ok_or(PalletError::StorageFetchFailed {
            pallet: "Staking",
            entry: "ValidatorCount",
        })
}

async fn fetch_force_era(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<ForceEra, PalletError> {
    staking_queries::get_force_era(client_at_block)
        .await
        .map(ForceEra::from)
        .ok_or(PalletError::StorageFetchFailed {
            pallet: "Staking",
            entry: "ForceEra",
        })
}

async fn fetch_active_era(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<ActiveEraInfo, PalletError> {
    staking_queries::get_active_era_info(client_at_block)
        .await
        .map(ActiveEraInfo::from)
        .ok_or(PalletError::ActiveEraNotFound)
}

async fn fetch_bonded_eras(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<(u32, u32)>, PalletError> {
    staking_queries::get_bonded_eras(client_at_block)
        .await
        .ok_or(PalletError::StorageFetchFailed {
            pallet: "Staking",
            entry: "BondedEras",
        })
}

async fn fetch_staking_validators(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    ss58_prefix: u16,
) -> Result<Vec<String>, PalletError> {
    // Use the centralized session validators query
    staking_queries::get_session_validators(client_at_block, ss58_prefix)
        .await
        .ok_or(PalletError::StorageFetchFailed {
            pallet: "Session",
            entry: "Validators",
        })
}

async fn fetch_unapplied_slashes(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    ss58_prefix: u16,
) -> Vec<UnappliedSlash> {
    // Use centralized iteration function
    let slashes = staking_queries::iter_unapplied_slashes(client_at_block).await;

    slashes
        .into_iter()
        .map(|slash| UnappliedSlash {
            era: slash.era.to_string(),
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
        })
        .collect()
}

async fn fetch_election_status(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Option<ElectionStatus> {
    staking_queries::get_era_election_status(client_at_block)
        .await
        .map(ElectionStatus::from)
}

// ============================================================================
// Session/Era Progress Derivation
// ============================================================================

async fn derive_session_era_progress_relay(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
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
    let sessions_per_era = get_sessions_per_era_from_metadata(&client_at_block.metadata()).ok_or(
        PalletError::ConstantNotFound {
            pallet: "Staking",
            constant: "SessionsPerEra",
        },
    )?;
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

async fn derive_session_era_progress_asset_hub(
    state: &AppState,
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<SessionEraProgress, PalletError> {
    let babe_params = get_asset_hub_babe_params(&state.chain_info.spec_name).ok_or_else(|| {
        PalletError::UnsupportedChainForStaking(state.chain_info.spec_name.clone())
    })?;

    // Fetch timestamp
    let timestamp = staking_queries::get_timestamp(client_at_block)
        .await
        .ok_or(PalletError::TimestampFetchFailed)?;

    // Fetch active era and bonded eras
    let active_era_info = fetch_active_era(client_at_block).await?;
    let bonded_eras = fetch_bonded_eras(client_at_block).await?;

    // Get sessions per era from metadata
    let sessions_per_era = get_sessions_per_era_from_metadata(&client_at_block.metadata()).ok_or(
        PalletError::ConstantNotFound {
            pallet: "Staking",
            constant: "SessionsPerEra",
        },
    )?;

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

    // Calculate session index accounting for skipped epochs from relay chain
    let current_index = if let Ok(relay_client) = state.get_relay_chain_client().await {
        // Get relay chain client at current block to fetch skipped epochs
        let relay_client_at_block = relay_client
            .at_current_block()
            .await
            .map_err(|e| RelayChainError::ConnectionFailed(e.to_string()))?;
        let skipped_epochs = fetch_relay_skipped_epochs(&relay_client_at_block).await;
        calculate_session_from_skipped_epochs(epoch_index, &skipped_epochs)
    } else {
        // Fallback: assume session = epoch if no relay chain configured
        epoch_index as u32
    };

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

/// Fetch skipped epochs from the relay chain's Babe pallet.
///
/// Skipped epochs occur when no blocks are produced in a slot window,
/// causing a discontinuity between epoch index and session index.
async fn fetch_relay_skipped_epochs(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Vec<(u64, u32)> {
    staking_queries::get_babe_skipped_epochs(client_at_block)
        .await
        .unwrap_or_default()
}

/// Calculate the session index from epoch index, accounting for skipped epochs.
///
/// When epochs are skipped, the session index doesn't advance even though
/// the epoch index does. This function uses the SkippedEpochs storage to
/// find the correct mapping.
fn calculate_session_from_skipped_epochs(epoch_index: u64, skipped_epochs: &[(u64, u32)]) -> u32 {
    if skipped_epochs.is_empty() {
        return epoch_index as u32;
    }

    // Sort by epoch index
    let mut sorted: Vec<_> = skipped_epochs.to_vec();
    sorted.sort_by_key(|(epoch, _)| *epoch);

    // Find closest skipped epoch <= current epoch
    let closest = sorted.iter().rfind(|(epoch, _)| *epoch <= epoch_index);

    match closest {
        Some((skipped_epoch, skipped_session)) => {
            let permanent_offset = skipped_epoch - (*skipped_session as u64);
            (epoch_index - permanent_offset) as u32
        }
        None => epoch_index as u32,
    }
}

async fn fetch_babe_current_slot(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<u64, PalletError> {
    staking_queries::get_babe_current_slot(client_at_block)
        .await
        .ok_or(PalletError::StorageFetchFailed {
            pallet: "Babe",
            entry: "CurrentSlot",
        })
}

async fn fetch_babe_epoch_index(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<u64, PalletError> {
    staking_queries::get_babe_epoch_index(client_at_block)
        .await
        .ok_or(PalletError::StorageFetchFailed {
            pallet: "Babe",
            entry: "EpochIndex",
        })
}

async fn fetch_babe_genesis_slot(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<u64, PalletError> {
    staking_queries::get_babe_genesis_slot(client_at_block)
        .await
        .ok_or(PalletError::StorageFetchFailed {
            pallet: "Babe",
            entry: "GenesisSlot",
        })
}

async fn fetch_session_current_index(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<u32, PalletError> {
    staking_queries::get_session_current_index(client_at_block)
        .await
        .ok_or(PalletError::StorageFetchFailed {
            pallet: "Session",
            entry: "CurrentIndex",
        })
}

fn get_sessions_per_era_from_metadata(metadata: &subxt::Metadata) -> Option<u32> {
    let pallet = metadata.pallet_by_name("Staking")?;
    let constant = pallet.constant_by_name("SessionsPerEra")?;
    u32::decode(&mut &constant.value()[..]).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_staking_progress_query_params_rejects_unknown_fields() {
        let json = r#"{"at": "12345", "unknownField": true}"#;
        let result: Result<StakingProgressQueryParams, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }

    #[test]
    fn test_rc_staking_progress_query_params_rejects_unknown_fields() {
        let json = r#"{"at": "12345", "unknownField": true}"#;
        let result: Result<RcStakingProgressQueryParams, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }
}
