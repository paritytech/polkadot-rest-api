use super::types::{
    BlockInfo, DecodedPoolAssetApproval, PoolAssetApprovalError, PoolAssetApprovalQueryParams,
    PoolAssetApprovalResponse,
};
use super::utils::validate_and_parse_address;
use crate::handlers::accounts::utils::{extract_u128_field, fetch_timestamp};
use crate::state::AppState;
use crate::utils::{self, find_ah_blocks_in_rc_block};
use axum::{
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
    Json,
};
use config::ChainType;
use scale_value::{Composite, Value, ValueDef};
use serde_json::json;
use sp_core::crypto::AccountId32;

// ================================================================================================
// Main Handler
// ================================================================================================

/// Handler for GET /accounts/{accountId}/pool-asset-approvals
///
/// Returns pool asset approval information for a given account, asset, and delegate.
///
/// Query Parameters:
/// - `at` (optional): Block identifier (hash or height) - defaults to latest finalized
/// - `useRcBlock` (optional): When true, treat 'at' as relay chain block identifier
/// - `assetId` (required): The pool asset ID to query approval for
/// - `delegate` (required): The delegate address with spending approval
pub async fn get_pool_asset_approvals(
    State(state): State<AppState>,
    Path(account_id): Path<String>,
    Query(params): Query<PoolAssetApprovalQueryParams>,
) -> Result<Response, PoolAssetApprovalError> {
    let account = validate_and_parse_address(&account_id)
        .map_err(|_| PoolAssetApprovalError::InvalidAddress(account_id.clone()))?;

    let delegate = validate_and_parse_address(&params.delegate)
        .map_err(|_| PoolAssetApprovalError::InvalidDelegateAddress(params.delegate.clone()))?;

    if params.use_rc_block {
        return handle_use_rc_block(state, account, delegate, params).await;
    }

    let block_id = params.at.map(|s| s.parse::<utils::BlockId>()).transpose()?;
    let resolved_block = utils::resolve_block(&state, block_id).await?;

    println!(
        "Fetching pool asset approval for account {:?} delegate {:?} asset_id {}",
        account, delegate, params.asset_id
    );

    let response = query_pool_asset_approval(
        &state,
        &account,
        &delegate,
        params.asset_id,
        &resolved_block,
    )
    .await?;

    Ok(Json(response).into_response())
}

async fn query_pool_asset_approval(
    state: &AppState,
    owner: &AccountId32,
    delegate: &AccountId32,
    asset_id: u32,
    block: &utils::ResolvedBlock,
) -> Result<PoolAssetApprovalResponse, PoolAssetApprovalError> {
    let client_at_block = state.client.at(block.number).await?;

    let approvals_exists = client_at_block
        .storage()
        .entry("PoolAssets", "Approvals")
        .is_ok();

    if !approvals_exists {
        return Err(PoolAssetApprovalError::PoolAssetsPalletNotAvailable);
    }

    let storage_entry = client_at_block
        .storage()
        .entry("PoolAssets", "Approvals")?;

    // Storage key for Approvals: (asset_id, owner, delegate)
    let owner_bytes: &[u8; 32] = owner.as_ref();
    let delegate_bytes: &[u8; 32] = delegate.as_ref();
    let key = (&asset_id, owner_bytes, delegate_bytes);

    let storage_value = storage_entry.fetch(&key).await?;

    let (amount, deposit) = if let Some(value) = storage_value {
        // Decode the approval
        let decoded = decode_pool_asset_approval(&value).await?;
        match decoded {
            Some(approval) => (
                Some(approval.amount.to_string()),
                Some(approval.deposit.to_string()),
            ),
            None => (None, None),
        }
    } else {
        (None, None)
    };

    Ok(PoolAssetApprovalResponse {
        at: BlockInfo {
            hash: block.hash.clone(),
            height: block.number.to_string(),
        },
        amount,
        deposit,
        rc_block_hash: None,
        rc_block_number: None,
        ah_timestamp: None,
    })
}

// ================================================================================================
// Pool Asset Approval Decoding
// ================================================================================================

/// Decode pool asset approval from storage value
async fn decode_pool_asset_approval(
    value: &subxt_historic::storage::StorageValue<'_>,
) -> Result<Option<DecodedPoolAssetApproval>, PoolAssetApprovalError> {
    // Decode as scale_value::Value to inspect structure
    let decoded: Value<()> = value.decode_as().map_err(|_e| {
        PoolAssetApprovalError::DecodeFailed(parity_scale_codec::Error::from(
            "Failed to decode storage value",
        ))
    })?;

    // Handle Option wrapper
    let approval_value = match &decoded.value {
        ValueDef::Variant(variant) => {
            if variant.name == "Some" {
                // Extract the inner value from the composite
                match &variant.values {
                    Composite::Unnamed(values) => {
                        if let Some(inner) = values.first() {
                            inner
                        } else {
                            return Ok(None);
                        }
                    }
                    Composite::Named(fields) => {
                        if let Some((_, inner)) = fields.first() {
                            inner
                        } else {
                            return Ok(None);
                        }
                    }
                }
            } else {
                // None variant
                return Ok(None);
            }
        }
        _ => &decoded,
    };

    // Now decode the actual approval structure
    match &approval_value.value {
        ValueDef::Composite(composite) => decode_approval_composite(composite),
        _ => Ok(None),
    }
}

/// Decode approval from a composite structure
fn decode_approval_composite(
    composite: &Composite<()>,
) -> Result<Option<DecodedPoolAssetApproval>, PoolAssetApprovalError> {
    match composite {
        Composite::Named(fields) => {
            // Extract amount and deposit fields
            let amount = extract_u128_field(fields, "amount").unwrap_or(0);
            let deposit = extract_u128_field(fields, "deposit").unwrap_or(0);

            Ok(Some(DecodedPoolAssetApproval { amount, deposit }))
        }
        Composite::Unnamed(values) => {
            // Some runtimes might use unnamed struct (tuple-like)
            // Approval is typically (amount, deposit) in order
            let amount = values.first().and_then(|v| match &v.value {
                ValueDef::Primitive(scale_value::Primitive::U128(val)) => Some(*val),
                _ => None,
            });
            let deposit = values.get(1).and_then(|v| match &v.value {
                ValueDef::Primitive(scale_value::Primitive::U128(val)) => Some(*val),
                _ => None,
            });

            match (amount, deposit) {
                (Some(amt), Some(dep)) => Ok(Some(DecodedPoolAssetApproval {
                    amount: amt,
                    deposit: dep,
                })),
                _ => Ok(None),
            }
        }
    }
}

// ================================================================================================
// Relay Chain Block Handling
// ================================================================================================

async fn handle_use_rc_block(
    state: AppState,
    account: AccountId32,
    delegate: AccountId32,
    params: PoolAssetApprovalQueryParams,
) -> Result<Response, PoolAssetApprovalError> {
    // Validate Asset Hub
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(PoolAssetApprovalError::UseRcBlockNotSupported);
    }

    if state.get_relay_chain_client().is_none() {
        return Err(PoolAssetApprovalError::RelayChainNotConfigured);
    }

    // Resolve RC block
    let rc_block_id = params
        .at
        .unwrap_or_else(|| "head".to_string())
        .parse::<utils::BlockId>()?;
    let rc_resolved = utils::resolve_block_with_rpc(
        state.get_relay_chain_rpc_client().unwrap(),
        state.get_relay_chain_rpc().unwrap(),
        Some(rc_block_id),
    )
    .await?;

    // Find AH blocks
    let ah_blocks = find_ah_blocks_in_rc_block(&state, &rc_resolved).await?;

    if ah_blocks.is_empty() {
        return Ok(Json(json!([])).into_response());
    }

    let rc_block_hash = rc_resolved.hash.clone();
    let rc_block_number = rc_resolved.number.to_string();

    // Process each AH block
    let mut results = Vec::new();
    for ah_block in ah_blocks {
        let ah_resolved = utils::ResolvedBlock {
            hash: ah_block.hash.clone(),
            number: ah_block.number,
        };

        let mut response = query_pool_asset_approval(
            &state,
            &account,
            &delegate,
            params.asset_id,
            &ah_resolved,
        )
        .await?;

        // Add RC block info
        response.rc_block_hash = Some(rc_block_hash.clone());
        response.rc_block_number = Some(rc_block_number.clone());

        // Fetch AH timestamp
        if let Ok(timestamp) = fetch_timestamp(&state, ah_block.number).await {
            response.ah_timestamp = Some(timestamp);
        }

        results.push(response);
    }

    Ok(Json(results).into_response())
}
