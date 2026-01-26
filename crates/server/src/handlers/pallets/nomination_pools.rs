//! Handler for the `/pallets/nomination-pools` endpoints.
//!
//! This module provides endpoints for querying nomination pool information
//! on Polkadot and Kusama networks.

use crate::handlers::pallets::common::{AtResponse, PalletError, format_account_id};
use crate::state::AppState;
use crate::utils;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use config::ChainType;
use parity_scale_codec::Decode;
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use subxt::{SubstrateConfig, client::OnlineClientAtBlock};

// ============================================================================
// Request/Response Types
// ============================================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
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

/// Handler for GET `/pallets/nomination-pools/info`
///
/// Returns global nomination pools statistics and configuration.
pub async fn pallets_nomination_pools_info(
    State(state): State<AppState>,
    Query(params): Query<NominationPoolsQueryParams>,
) -> Result<Response, PalletError> {
    // Check if chain supports nomination pools (not Asset Hub)
    if state.chain_info.chain_type == ChainType::AssetHub {
        return Err(PalletError::UnsupportedChainForStaking(
            "Nomination pools are not available on Asset Hub".to_string(),
        ));
    }

    // Handle useRcBlock mode - not typically used for nomination pools but supported for consistency
    if params.use_rc_block {
        return Err(PalletError::UseRcBlockNotSupported);
    }

    // Create client at the specified block
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

    // Fetch all nomination pools info storage values
    let counter_for_bonded_pools =
        fetch_storage_value_u32(&client_at_block, "NominationPools", "CounterForBondedPools")
            .await
            .unwrap_or(0);
    let counter_for_metadata =
        fetch_storage_value_u32(&client_at_block, "NominationPools", "CounterForMetadata")
            .await
            .unwrap_or(0);
    let counter_for_pool_members =
        fetch_storage_value_u32(&client_at_block, "NominationPools", "CounterForPoolMembers")
            .await
            .unwrap_or(0);
    let counter_for_reverse_pool_id_lookup = fetch_storage_value_u32(
        &client_at_block,
        "NominationPools",
        "CounterForReversePoolIdLookup",
    )
    .await
    .unwrap_or(0);
    let counter_for_reward_pools =
        fetch_storage_value_u32(&client_at_block, "NominationPools", "CounterForRewardPools")
            .await
            .unwrap_or(0);
    let counter_for_sub_pools_storage = fetch_storage_value_u32(
        &client_at_block,
        "NominationPools",
        "CounterForSubPoolsStorage",
    )
    .await
    .unwrap_or(0);
    let last_pool_id = fetch_storage_value_u32(&client_at_block, "NominationPools", "LastPoolId")
        .await
        .unwrap_or(0);
    let max_pool_members =
        fetch_storage_value_option_u32(&client_at_block, "NominationPools", "MaxPoolMembers").await;
    let max_pool_members_per_pool = fetch_storage_value_option_u32(
        &client_at_block,
        "NominationPools",
        "MaxPoolMembersPerPool",
    )
    .await;
    let max_pools =
        fetch_storage_value_option_u32(&client_at_block, "NominationPools", "MaxPools").await;
    let min_create_bond =
        fetch_storage_value_u128(&client_at_block, "NominationPools", "MinCreateBond")
            .await
            .unwrap_or(0);
    let min_join_bond =
        fetch_storage_value_u128(&client_at_block, "NominationPools", "MinJoinBond")
            .await
            .unwrap_or(0);

    Ok((
        StatusCode::OK,
        Json(NominationPoolsInfoResponse {
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
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        }),
    )
        .into_response())
}

/// Handler for GET `/pallets/nomination-pools/{poolId}`
///
/// Returns details for a specific nomination pool.
pub async fn pallets_nomination_pools_pool(
    State(state): State<AppState>,
    Path(pool_id): Path<String>,
    Query(params): Query<NominationPoolsQueryParams>,
) -> Result<Response, PalletError> {
    // Parse pool ID
    let pool_id: u32 = pool_id
        .parse()
        .map_err(|_| PalletError::PoolNotFound(format!("Invalid pool ID: {}", pool_id)))?;

    // Check if chain supports nomination pools (not Asset Hub)
    if state.chain_info.chain_type == ChainType::AssetHub {
        return Err(PalletError::UnsupportedChainForStaking(
            "Nomination pools are not available on Asset Hub".to_string(),
        ));
    }

    // Handle useRcBlock mode - not typically used for nomination pools but supported for consistency
    if params.use_rc_block {
        return Err(PalletError::UseRcBlockNotSupported);
    }

    // Create client at the specified block
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

    let ss58_prefix = state.chain_info.ss58_prefix;

    // Fetch bonded pool - try V2 format first (with commission), then fall back to V1
    let bonded_pool = fetch_bonded_pool(&client_at_block, pool_id, ss58_prefix).await;

    // Fetch reward pool - try V2 format first, then fall back to V1
    let reward_pool = fetch_reward_pool(&client_at_block, pool_id).await;

    // Note: Sidecar returns null for both fields when pool doesn't exist (no 404)
    // We match this behavior

    Ok((
        StatusCode::OK,
        Json(NominationPoolResponse {
            at,
            bonded_pool,
            reward_pool,
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        }),
    )
        .into_response())
}

// ============================================================================
// Helper Functions - Storage Value Fetchers
// ============================================================================

/// Fetches a u32 storage value.
async fn fetch_storage_value_u32(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    pallet: &str,
    entry: &str,
) -> Option<u32> {
    let addr = subxt::dynamic::storage::<(), u32>(pallet, entry);
    let value = client_at_block.storage().fetch(addr, ()).await.ok()?;
    value.decode().ok()
}

/// Fetches an Option<u32> storage value (for max values that can be None).
async fn fetch_storage_value_option_u32(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    pallet: &str,
    entry: &str,
) -> Option<u32> {
    let addr = subxt::dynamic::storage::<(), Option<u32>>(pallet, entry);
    let value = client_at_block.storage().fetch(addr, ()).await.ok()?;
    value.decode().ok().flatten()
}

/// Fetches a u128 storage value.
async fn fetch_storage_value_u128(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    pallet: &str,
    entry: &str,
) -> Option<u128> {
    let addr = subxt::dynamic::storage::<(), u128>(pallet, entry);
    let value = client_at_block.storage().fetch(addr, ()).await.ok()?;
    value.decode().ok()
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
    if let Ok(storage) = BondedPoolStorageV2::decode(&mut &raw_bytes[..]) {
        return Some(bonded_pool_v2_to_json(&storage, ss58_prefix));
    }

    // Fall back to V1 format (legacy without commission)
    if let Ok(storage) = BondedPoolStorageV1::decode(&mut &raw_bytes[..]) {
        return Some(bonded_pool_v1_to_json(&storage, ss58_prefix));
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
            "throttleFrom": storage.commission.throttle_from,
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
    if let Ok(storage) = RewardPoolStorageV2::decode(&mut &raw_bytes[..]) {
        return Some(json!({
            "lastRecordedRewardCounter": storage.last_recorded_reward_counter.to_string(),
            "lastRecordedTotalPayouts": storage.last_recorded_total_payouts.to_string(),
            "totalRewardsClaimed": storage.total_rewards_claimed.to_string(),
            "totalCommissionPending": storage.total_commission_pending.to_string(),
            "totalCommissionClaimed": storage.total_commission_claimed.to_string()
        }));
    }

    // Fall back to V1 format (legacy without commission)
    if let Ok(storage) = RewardPoolStorageV1::decode(&mut &raw_bytes[..]) {
        return Some(json!({
            "lastRecordedRewardCounter": storage.last_recorded_reward_counter.to_string(),
            "lastRecordedTotalPayouts": storage.last_recorded_total_payouts.to_string(),
            "totalRewardsClaimed": storage.total_rewards_claimed.to_string(),
            "totalCommissionPending": "0",
            "totalCommissionClaimed": "0"
        }));
    }

    None
}
