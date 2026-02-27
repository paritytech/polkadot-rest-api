// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Handler for the `/pallets/nomination-pools` endpoints.
//!
//! This module provides endpoints for querying nomination pool information
//! on Polkadot and Kusama networks.

use crate::extractors::JsonQuery;
use crate::handlers::pallets::common::{
    AtResponse, PalletError, format_account_id, resolve_block_for_pallet,
};
use crate::state::AppState;
use crate::utils;
use crate::utils::rc_block::find_ah_blocks_in_rc_block;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use parity_scale_codec::Decode;
use polkadot_rest_api_config::ChainType;
use scale_decode::DecodeAsType;
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use subxt::{SubstrateConfig, client::OnlineClientAtBlock};

// ============================================================================
// Request/Response Types
// ============================================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NominationPoolsQueryParams {
    pub at: Option<String>,
    #[serde(default)]
    pub use_rc_block: bool,
}

/// Response for `/pallets/nomination-pools/info`
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NominationPoolsInfoResponse {
    pub at: AtResponse,
    pub counter_for_bonded_pools: String,
    pub counter_for_metadata: String,
    pub counter_for_pool_members: String,
    pub counter_for_reverse_pool_id_lookup: String,
    pub counter_for_reward_pools: String,
    pub counter_for_sub_pools_storage: String,
    pub last_pool_id: String,
    pub max_pool_members: Option<u32>,
    pub max_pool_members_per_pool: Option<u32>,
    pub max_pools: Option<u32>,
    pub min_create_bond: String,
    pub min_join_bond: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}

/// Response for `/pallets/nomination-pools/{poolId}`
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NominationPoolResponse {
    pub at: AtResponse,
    pub bonded_pool: Option<JsonValue>,
    pub reward_pool: Option<JsonValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}

// ============================================================================
// Internal SCALE Decode Types
// ============================================================================

#[derive(Debug, Clone, Decode)]
enum PoolState {
    Open,
    Blocked,
    Destroying,
}

impl PoolState {
    fn as_str(&self) -> &'static str {
        match self {
            PoolState::Open => "Open",
            PoolState::Blocked => "Blocked",
            PoolState::Destroying => "Destroying",
        }
    }
}

/// Bonded pool storage format (modern version with commission)
#[derive(Debug, Clone, Decode)]
struct BondedPoolStorageV2 {
    commission: CommissionStorage,
    member_counter: u32,
    points: u128,
    roles: PoolRolesStorage,
    state: PoolState,
}

/// Bonded pool storage format (legacy without commission)
#[derive(Debug, Clone, Decode)]
struct BondedPoolStorageV1 {
    points: u128,
    state: PoolState,
    member_counter: u32,
    roles: PoolRolesStorage,
}

#[derive(Debug, Clone, Decode)]
struct CommissionStorage {
    current: Option<(u32, [u8; 32])>, // (Perbill, AccountId)
    max: Option<u32>,                 // Perbill
    change_rate: Option<CommissionChangeRate>,
    throttle_from: Option<u32>, // BlockNumber
    claim_permission: Option<CommissionClaimPermission>,
}

#[derive(Debug, Clone, Decode)]
struct CommissionChangeRate {
    max_increase: u32, // Perbill
    min_delay: u32,    // BlockNumber
}

#[derive(Debug, Clone, Decode)]
enum CommissionClaimPermission {
    Permissionless,
    #[allow(dead_code)]
    Account([u8; 32]),
}

#[derive(Debug, Clone, Decode)]
struct PoolRolesStorage {
    depositor: [u8; 32],
    root: Option<[u8; 32]>,
    nominator: Option<[u8; 32]>,
    bouncer: Option<[u8; 32]>,
}

/// Reward pool storage format (modern with commission)
#[derive(Debug, Clone, Decode)]
struct RewardPoolStorageV2 {
    last_recorded_reward_counter: u128,
    last_recorded_total_payouts: u128,
    total_rewards_claimed: u128,
    total_commission_pending: u128,
    total_commission_claimed: u128,
}

/// Reward pool storage format (legacy without commission)
#[derive(Debug, Clone, Decode)]
struct RewardPoolStorageV1 {
    last_recorded_reward_counter: u128,
    last_recorded_total_payouts: u128,
    total_rewards_claimed: u128,
}

// ============================================================================
// Main Handlers
// ============================================================================

#[utoipa::path(
    get,
    path = "/v1/pallets/nomination-pools/info",
    tag = "pallets",
    summary = "Nomination pools info",
    description = "Returns global nomination pools statistics and configuration.",
    params(
        ("at" = Option<String>, description = "Block hash or number to query at"),
        ("useRcBlock" = Option<bool>, description = "Treat 'at' as relay chain block identifier")
    ),
    responses(
        (status = 200, description = "Nomination pools information", body = Object),
        (status = 400, description = "Not supported on this chain"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn pallets_nomination_pools_info(
    State(state): State<AppState>,
    JsonQuery(params): JsonQuery<NominationPoolsQueryParams>,
) -> Result<Response, PalletError> {
    if params.use_rc_block {
        return handle_info_use_rc_block(state, params).await;
    }

    // Check if chain supports nomination pools (not Asset Hub)
    if state.chain_info.chain_type == ChainType::AssetHub {
        return Err(PalletError::UnsupportedChainForStaking(
            "Nomination pools are not available on Asset Hub".to_string(),
        ));
    }

    // Resolve block using the common helper
    let resolved = resolve_block_for_pallet(&state.client, params.at.as_ref()).await?;

    let response =
        build_nomination_pools_info(&resolved.client_at_block, resolved.at, None, None, None).await;

    Ok((StatusCode::OK, Json(response)).into_response())
}

#[utoipa::path(
    get,
    path = "/v1/pallets/nomination-pools/{poolId}",
    tag = "pallets",
    summary = "Nomination pool details",
    description = "Returns details for a specific nomination pool.",
    params(
        ("poolId" = String, Path, description = "Pool ID"),
        ("at" = Option<String>, description = "Block hash or number to query at"),
        ("useRcBlock" = Option<bool>, description = "Treat 'at' as relay chain block identifier")
    ),
    responses(
        (status = 200, description = "Pool details", body = Object),
        (status = 404, description = "Pool not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn pallets_nomination_pools_pool(
    State(state): State<AppState>,
    Path(pool_id): Path<String>,
    JsonQuery(params): JsonQuery<NominationPoolsQueryParams>,
) -> Result<Response, PalletError> {
    // Parse pool ID
    let pool_id: u32 = pool_id
        .parse()
        .map_err(|_| PalletError::PoolNotFound(format!("Invalid pool ID: {}", pool_id)))?;

    if params.use_rc_block {
        return handle_pool_use_rc_block(state, pool_id, params).await;
    }

    // Check if chain supports nomination pools (not Asset Hub)
    if state.chain_info.chain_type == ChainType::AssetHub {
        return Err(PalletError::UnsupportedChainForStaking(
            "Nomination pools are not available on Asset Hub".to_string(),
        ));
    }

    // Resolve block using the common helper
    let resolved = resolve_block_for_pallet(&state.client, params.at.as_ref()).await?;

    let ss58_prefix = state.chain_info.ss58_prefix;

    // Fetch bonded pool - try V2 format first (with commission), then fall back to V1
    let bonded_pool = fetch_bonded_pool(&resolved.client_at_block, pool_id, ss58_prefix).await;

    // Fetch reward pool - try V2 format first, then fall back to V1
    let reward_pool = fetch_reward_pool(&resolved.client_at_block, pool_id).await;

    // Note: Sidecar returns null for both fields when pool doesn't exist (no 404)
    // We match this behavior

    Ok((
        StatusCode::OK,
        Json(NominationPoolResponse {
            at: resolved.at,
            bonded_pool,
            reward_pool,
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        }),
    )
        .into_response())
}

async fn handle_info_use_rc_block(
    state: AppState,
    params: NominationPoolsQueryParams,
) -> Result<Response, PalletError> {
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(PalletError::UseRcBlockNotSupported);
    }

    state.get_relay_chain_client().await?;

    let rc_block_id = params
        .at
        .as_ref()
        .ok_or(PalletError::AtParameterRequired)?
        .parse::<utils::BlockId>()?;

    let rc_rpc_client = state.get_relay_chain_rpc_client().await?;
    let rc_rpc = state.get_relay_chain_rpc().await?;

    let rc_resolved_block =
        utils::resolve_block_with_rpc(&rc_rpc_client, &rc_rpc, Some(rc_block_id)).await?;

    let ah_blocks = find_ah_blocks_in_rc_block(&state, &rc_resolved_block).await?;

    if ah_blocks.is_empty() {
        return Ok((
            StatusCode::OK,
            Json(Vec::<NominationPoolsInfoResponse>::new()),
        )
            .into_response());
    }

    let mut results = Vec::new();
    let rc_block_hash = rc_resolved_block.hash.clone();
    let rc_block_number = rc_resolved_block.number.to_string();

    for ah_block in &ah_blocks {
        let client_at_block = state.client.at_block(ah_block.number).await?;

        let at = AtResponse {
            hash: ah_block.hash.clone(),
            height: ah_block.number.to_string(),
        };

        let ah_timestamp = utils::fetch_block_timestamp(&client_at_block).await;

        let response = build_nomination_pools_info(
            &client_at_block,
            at,
            Some(rc_block_hash.clone()),
            Some(rc_block_number.clone()),
            ah_timestamp,
        )
        .await;

        results.push(response);
    }

    Ok((StatusCode::OK, Json(results)).into_response())
}

async fn handle_pool_use_rc_block(
    state: AppState,
    pool_id: u32,
    params: NominationPoolsQueryParams,
) -> Result<Response, PalletError> {
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(PalletError::UseRcBlockNotSupported);
    }

    state.get_relay_chain_client().await?;

    let rc_block_id = params
        .at
        .as_ref()
        .ok_or(PalletError::AtParameterRequired)?
        .parse::<utils::BlockId>()?;

    let rc_rpc_client = state.get_relay_chain_rpc_client().await?;
    let rc_rpc = state.get_relay_chain_rpc().await?;

    let rc_resolved_block =
        utils::resolve_block_with_rpc(&rc_rpc_client, &rc_rpc, Some(rc_block_id)).await?;

    let ah_blocks = find_ah_blocks_in_rc_block(&state, &rc_resolved_block).await?;

    if ah_blocks.is_empty() {
        return Ok((StatusCode::OK, Json(Vec::<NominationPoolResponse>::new())).into_response());
    }

    let mut results = Vec::new();
    let rc_block_hash = rc_resolved_block.hash.clone();
    let rc_block_number = rc_resolved_block.number.to_string();
    let ss58_prefix = state.chain_info.ss58_prefix;

    for ah_block in &ah_blocks {
        let client_at_block = state.client.at_block(ah_block.number).await?;

        let at = AtResponse {
            hash: ah_block.hash.clone(),
            height: ah_block.number.to_string(),
        };

        let ah_timestamp = utils::fetch_block_timestamp(&client_at_block).await;

        let bonded_pool = fetch_bonded_pool(&client_at_block, pool_id, ss58_prefix).await;
        let reward_pool = fetch_reward_pool(&client_at_block, pool_id).await;

        results.push(NominationPoolResponse {
            at,
            bonded_pool,
            reward_pool,
            rc_block_hash: Some(rc_block_hash.clone()),
            rc_block_number: Some(rc_block_number.clone()),
            ah_timestamp,
        });
    }

    Ok((StatusCode::OK, Json(results)).into_response())
}

async fn build_nomination_pools_info(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    at: AtResponse,
    rc_block_hash: Option<String>,
    rc_block_number: Option<String>,
    ah_timestamp: Option<String>,
) -> NominationPoolsInfoResponse {
    let counter_for_bonded_pools =
        fetch_storage_value::<u32>(client_at_block, "NominationPools", "CounterForBondedPools")
            .await
            .unwrap_or(0);
    let counter_for_metadata =
        fetch_storage_value::<u32>(client_at_block, "NominationPools", "CounterForMetadata")
            .await
            .unwrap_or(0);
    let counter_for_pool_members =
        fetch_storage_value::<u32>(client_at_block, "NominationPools", "CounterForPoolMembers")
            .await
            .unwrap_or(0);
    let counter_for_reverse_pool_id_lookup = fetch_storage_value::<u32>(
        client_at_block,
        "NominationPools",
        "CounterForReversePoolIdLookup",
    )
    .await
    .unwrap_or(0);
    let counter_for_reward_pools =
        fetch_storage_value::<u32>(client_at_block, "NominationPools", "CounterForRewardPools")
            .await
            .unwrap_or(0);
    let counter_for_sub_pools_storage = fetch_storage_value::<u32>(
        client_at_block,
        "NominationPools",
        "CounterForSubPoolsStorage",
    )
    .await
    .unwrap_or(0);
    let last_pool_id = fetch_storage_value::<u32>(client_at_block, "NominationPools", "LastPoolId")
        .await
        .unwrap_or(0);
    let max_pool_members =
        fetch_storage_value::<Option<u32>>(client_at_block, "NominationPools", "MaxPoolMembers")
            .await
            .flatten();
    let max_pool_members_per_pool = fetch_storage_value::<Option<u32>>(
        client_at_block,
        "NominationPools",
        "MaxPoolMembersPerPool",
    )
    .await
    .flatten();
    let max_pools =
        fetch_storage_value::<Option<u32>>(client_at_block, "NominationPools", "MaxPools")
            .await
            .flatten();
    let min_create_bond =
        fetch_storage_value::<u128>(client_at_block, "NominationPools", "MinCreateBond")
            .await
            .unwrap_or(0);
    let min_join_bond =
        fetch_storage_value::<u128>(client_at_block, "NominationPools", "MinJoinBond")
            .await
            .unwrap_or(0);

    NominationPoolsInfoResponse {
        at,
        counter_for_bonded_pools: counter_for_bonded_pools.to_string(),
        counter_for_metadata: counter_for_metadata.to_string(),
        counter_for_pool_members: counter_for_pool_members.to_string(),
        counter_for_reverse_pool_id_lookup: counter_for_reverse_pool_id_lookup.to_string(),
        counter_for_reward_pools: counter_for_reward_pools.to_string(),
        counter_for_sub_pools_storage: counter_for_sub_pools_storage.to_string(),
        last_pool_id: last_pool_id.to_string(),
        max_pool_members,
        max_pool_members_per_pool,
        max_pools,
        min_create_bond: min_create_bond.to_string(),
        min_join_bond: min_join_bond.to_string(),
        rc_block_hash,
        rc_block_number,
        ah_timestamp,
    }
}

// ============================================================================
// Helper Functions - Storage Value Fetchers
// ============================================================================

/// Generic function to fetch and decode a storage value.
/// Uses `DecodeAsType` for type-guided decoding.
async fn fetch_storage_value<T>(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    pallet: &str,
    entry: &str,
) -> Option<T>
where
    T: DecodeAsType,
{
    let addr = subxt::dynamic::storage::<(), scale_value::Value>(pallet, entry);
    let storage_value = client_at_block.storage().fetch(addr, ()).await.ok()?;
    storage_value.decode_as().ok()
}

// ============================================================================
// Helper Functions - Pool Data Fetchers
// ============================================================================

/// Fetches bonded pool details from NominationPools::BondedPools storage.
async fn fetch_bonded_pool(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    pool_id: u32,
    ss58_prefix: u16,
) -> Option<JsonValue> {
    let addr = subxt::dynamic::storage::<_, scale_value::Value>("NominationPools", "BondedPools");
    let raw_bytes = match client_at_block.storage().fetch(addr, (pool_id,)).await {
        Ok(value) => value.into_bytes(),
        Err(_) => return None,
    };

    // Try modern V2 format first (with commission)
    // Using raw Decode since we're confident about the exact byte layout
    let mut cursor = &raw_bytes[..];
    if let Ok(storage) = BondedPoolStorageV2::decode(&mut cursor) {
        // Sanity check: ensure all bytes were consumed
        if cursor.is_empty() {
            return Some(bonded_pool_v2_to_json(&storage, ss58_prefix));
        }
    }

    // Fall back to V1 format (legacy without commission)
    let mut cursor = &raw_bytes[..];
    if let Ok(storage) = BondedPoolStorageV1::decode(&mut cursor) {
        // Sanity check: ensure all bytes were consumed
        if cursor.is_empty() {
            return Some(bonded_pool_v1_to_json(&storage, ss58_prefix));
        }
    }

    None
}

/// Converts V2 bonded pool storage to JSON matching Sidecar output format.
fn bonded_pool_v2_to_json(storage: &BondedPoolStorageV2, ss58_prefix: u16) -> JsonValue {
    json!({
        "commission": {
            "current": storage.commission.current.as_ref().map(|(perbill, _account)| {
                // Convert Perbill to percentage string
                json!([perbill.to_string(), null])
            }),
            "max": storage.commission.max.map(|p| p.to_string()),
            "changeRate": storage.commission.change_rate.as_ref().map(|cr| {
                json!({
                    "maxIncrease": cr.max_increase.to_string(),
                    "minDelay": cr.min_delay.to_string()
                })
            }),
            "throttleFrom": storage.commission.throttle_from.map(|t| t.to_string()),
            "claimPermission": match &storage.commission.claim_permission {
                Some(CommissionClaimPermission::Permissionless) => Some("Permissionless"),
                Some(CommissionClaimPermission::Account(_)) => Some("Account"),
                None => None,
            }
        },
        "memberCounter": storage.member_counter.to_string(),
        "points": storage.points.to_string(),
        "roles": {
            "depositor": format_account_id(&storage.roles.depositor, ss58_prefix),
            "root": storage.roles.root.as_ref().map(|a| format_account_id(a, ss58_prefix)),
            "nominator": storage.roles.nominator.as_ref().map(|a| format_account_id(a, ss58_prefix)),
            "bouncer": storage.roles.bouncer.as_ref().map(|a| format_account_id(a, ss58_prefix))
        },
        "state": storage.state.as_str()
    })
}

/// Converts V1 bonded pool storage to JSON matching Sidecar output format.
fn bonded_pool_v1_to_json(storage: &BondedPoolStorageV1, ss58_prefix: u16) -> JsonValue {
    json!({
        "commission": {
            "current": null,
            "max": null,
            "changeRate": null,
            "throttleFrom": null,
            "claimPermission": null
        },
        "memberCounter": storage.member_counter.to_string(),
        "points": storage.points.to_string(),
        "roles": {
            "depositor": format_account_id(&storage.roles.depositor, ss58_prefix),
            "root": storage.roles.root.as_ref().map(|a| format_account_id(a, ss58_prefix)),
            "nominator": storage.roles.nominator.as_ref().map(|a| format_account_id(a, ss58_prefix)),
            "bouncer": storage.roles.bouncer.as_ref().map(|a| format_account_id(a, ss58_prefix))
        },
        "state": storage.state.as_str()
    })
}

/// Fetches reward pool details from NominationPools::RewardPools storage.
async fn fetch_reward_pool(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    pool_id: u32,
) -> Option<JsonValue> {
    let addr = subxt::dynamic::storage::<_, scale_value::Value>("NominationPools", "RewardPools");
    let raw_bytes = match client_at_block.storage().fetch(addr, (pool_id,)).await {
        Ok(value) => value.into_bytes(),
        Err(_) => return None,
    };

    // Try modern V2 format first (with commission tracking)
    // Using raw Decode since we're confident about the exact byte layout
    let mut cursor = &raw_bytes[..];
    if let Ok(storage) = RewardPoolStorageV2::decode(&mut cursor) {
        // Sanity check: ensure all bytes were consumed
        if cursor.is_empty() {
            return Some(json!({
                "lastRecordedRewardCounter": storage.last_recorded_reward_counter.to_string(),
                "lastRecordedTotalPayouts": storage.last_recorded_total_payouts.to_string(),
                "totalRewardsClaimed": storage.total_rewards_claimed.to_string(),
                "totalCommissionPending": storage.total_commission_pending.to_string(),
                "totalCommissionClaimed": storage.total_commission_claimed.to_string()
            }));
        }
    }

    // Fall back to V1 format (legacy without commission)
    let mut cursor = &raw_bytes[..];
    if let Ok(storage) = RewardPoolStorageV1::decode(&mut cursor) {
        // Sanity check: ensure all bytes were consumed
        if cursor.is_empty() {
            return Some(json!({
                "lastRecordedRewardCounter": storage.last_recorded_reward_counter.to_string(),
                "lastRecordedTotalPayouts": storage.last_recorded_total_payouts.to_string(),
                "totalRewardsClaimed": storage.total_rewards_claimed.to_string(),
                "totalCommissionPending": "0",
                "totalCommissionClaimed": "0"
            }));
        }
    }

    None
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_state_as_str() {
        assert_eq!(PoolState::Open.as_str(), "Open");
        assert_eq!(PoolState::Blocked.as_str(), "Blocked");
        assert_eq!(PoolState::Destroying.as_str(), "Destroying");
    }

    #[test]
    fn test_nomination_pools_info_response_serialization() {
        let response = NominationPoolsInfoResponse {
            at: AtResponse {
                hash: "0x123abc".to_string(),
                height: "1000000".to_string(),
            },
            counter_for_bonded_pools: "100".to_string(),
            counter_for_metadata: "50".to_string(),
            counter_for_pool_members: "5000".to_string(),
            counter_for_reverse_pool_id_lookup: "100".to_string(),
            counter_for_reward_pools: "100".to_string(),
            counter_for_sub_pools_storage: "100".to_string(),
            last_pool_id: "100".to_string(),
            max_pool_members: Some(50000),
            max_pool_members_per_pool: Some(1000),
            max_pools: Some(500),
            min_create_bond: "1000000000000".to_string(),
            min_join_bond: "100000000000".to_string(),
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        };

        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["at"]["hash"], "0x123abc");
        assert_eq!(json["at"]["height"], "1000000");
        assert_eq!(json["counterForBondedPools"], "100");
        assert_eq!(json["counterForMetadata"], "50");
        assert_eq!(json["counterForPoolMembers"], "5000");
        assert_eq!(json["lastPoolId"], "100");
        assert_eq!(json["maxPoolMembers"], 50000);
        assert_eq!(json["maxPoolMembersPerPool"], 1000);
        assert_eq!(json["maxPools"], 500);
        assert_eq!(json["minCreateBond"], "1000000000000");
        assert_eq!(json["minJoinBond"], "100000000000");
        // Optional fields should not be present when None
        assert!(json.get("rcBlockHash").is_none());
        assert!(json.get("rcBlockNumber").is_none());
        assert!(json.get("ahTimestamp").is_none());
    }

    #[test]
    fn test_nomination_pools_info_response_with_rc_block() {
        let response = NominationPoolsInfoResponse {
            at: AtResponse {
                hash: "0xabc".to_string(),
                height: "500".to_string(),
            },
            counter_for_bonded_pools: "10".to_string(),
            counter_for_metadata: "10".to_string(),
            counter_for_pool_members: "100".to_string(),
            counter_for_reverse_pool_id_lookup: "10".to_string(),
            counter_for_reward_pools: "10".to_string(),
            counter_for_sub_pools_storage: "10".to_string(),
            last_pool_id: "10".to_string(),
            max_pool_members: None,
            max_pool_members_per_pool: None,
            max_pools: None,
            min_create_bond: "1000".to_string(),
            min_join_bond: "100".to_string(),
            rc_block_hash: Some("0xrc123".to_string()),
            rc_block_number: Some("999".to_string()),
            ah_timestamp: Some("1234567890".to_string()),
        };

        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["rcBlockHash"], "0xrc123");
        assert_eq!(json["rcBlockNumber"], "999");
        assert_eq!(json["ahTimestamp"], "1234567890");
        // Optional config fields should be null when None
        assert!(json["maxPoolMembers"].is_null());
        assert!(json["maxPoolMembersPerPool"].is_null());
        assert!(json["maxPools"].is_null());
    }

    #[test]
    fn test_nomination_pool_response_serialization() {
        let response = NominationPoolResponse {
            at: AtResponse {
                hash: "0xdef456".to_string(),
                height: "2000000".to_string(),
            },
            bonded_pool: Some(json!({
                "commission": {
                    "current": null,
                    "max": null,
                    "changeRate": null,
                    "throttleFrom": null,
                    "claimPermission": null
                },
                "memberCounter": "25",
                "points": "1000000000000",
                "roles": {
                    "depositor": "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
                    "root": "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
                    "nominator": null,
                    "bouncer": null
                },
                "state": "Open"
            })),
            reward_pool: Some(json!({
                "lastRecordedRewardCounter": "12345678901234567890",
                "lastRecordedTotalPayouts": "1000000000000",
                "totalRewardsClaimed": "500000000000",
                "totalCommissionPending": "0",
                "totalCommissionClaimed": "0"
            })),
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        };

        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["at"]["hash"], "0xdef456");
        assert_eq!(json["at"]["height"], "2000000");
        assert!(json["bondedPool"].is_object());
        assert_eq!(json["bondedPool"]["memberCounter"], "25");
        assert_eq!(json["bondedPool"]["state"], "Open");
        assert!(json["rewardPool"].is_object());
        assert_eq!(json["rewardPool"]["totalRewardsClaimed"], "500000000000");
    }

    #[test]
    fn test_nomination_pool_response_with_null_pools() {
        let response = NominationPoolResponse {
            at: AtResponse {
                hash: "0x000".to_string(),
                height: "1".to_string(),
            },
            bonded_pool: None,
            reward_pool: None,
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        };

        let json = serde_json::to_value(&response).unwrap();
        assert!(json["bondedPool"].is_null());
        assert!(json["rewardPool"].is_null());
    }

    #[test]
    fn test_query_params_deserialization() {
        // Test with all fields
        let json = r#"{"at": "12345", "useRcBlock": true}"#;
        let params: NominationPoolsQueryParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.at, Some("12345".to_string()));
        assert!(params.use_rc_block);

        // Test with defaults
        let json = r#"{}"#;
        let params: NominationPoolsQueryParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.at, None);
        assert!(!params.use_rc_block);

        // Test with only at
        let json = r#"{"at": "0xabc123"}"#;
        let params: NominationPoolsQueryParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.at, Some("0xabc123".to_string()));
        assert!(!params.use_rc_block);
    }

    #[test]
    fn test_query_params_camel_case() {
        // Verify camelCase is used for useRcBlock
        let json = r#"{"useRcBlock": true}"#;
        let params: NominationPoolsQueryParams = serde_json::from_str(json).unwrap();
        assert!(params.use_rc_block);

        // snake_case should NOT work due to rename_all = "camelCase" + deny_unknown_fields
        let json = r#"{"use_rc_block": true}"#;
        let result: Result<NominationPoolsQueryParams, _> = serde_json::from_str(json);
        assert!(result.is_err(), "snake_case field should be rejected");
    }

    #[test]
    fn test_nomination_pools_query_params_rejects_unknown_fields() {
        let json = r#"{"at": "12345", "unknownField": true}"#;
        let result: Result<NominationPoolsQueryParams, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }

    #[test]
    fn test_response_camel_case_field_names() {
        let response = NominationPoolsInfoResponse {
            at: AtResponse {
                hash: "0x1".to_string(),
                height: "1".to_string(),
            },
            counter_for_bonded_pools: "1".to_string(),
            counter_for_metadata: "1".to_string(),
            counter_for_pool_members: "1".to_string(),
            counter_for_reverse_pool_id_lookup: "1".to_string(),
            counter_for_reward_pools: "1".to_string(),
            counter_for_sub_pools_storage: "1".to_string(),
            last_pool_id: "1".to_string(),
            max_pool_members: None,
            max_pool_members_per_pool: None,
            max_pools: None,
            min_create_bond: "1".to_string(),
            min_join_bond: "1".to_string(),
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        };

        let json_str = serde_json::to_string(&response).unwrap();

        // Verify camelCase field names in serialized JSON
        assert!(json_str.contains("counterForBondedPools"));
        assert!(json_str.contains("counterForMetadata"));
        assert!(json_str.contains("counterForPoolMembers"));
        assert!(json_str.contains("counterForReversePoolIdLookup"));
        assert!(json_str.contains("counterForRewardPools"));
        assert!(json_str.contains("counterForSubPoolsStorage"));
        assert!(json_str.contains("lastPoolId"));
        assert!(json_str.contains("minCreateBond"));
        assert!(json_str.contains("minJoinBond"));

        // Verify snake_case is NOT used
        assert!(!json_str.contains("counter_for_bonded_pools"));
        assert!(!json_str.contains("last_pool_id"));
    }
}
