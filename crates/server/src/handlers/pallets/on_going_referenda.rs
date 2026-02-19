// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Handler for the `/pallets/on-going-referenda` endpoint.
//!
//! This endpoint returns all currently active (ongoing) referenda from the
//! Referenda pallet. Only relay chains (Polkadot, Kusama) support this endpoint
//! as parachains don't have governance.

use crate::handlers::pallets::common::{
    AtResponse, ClientAtBlock, PalletError, format_account_id, resolve_block_for_pallet,
};
use crate::state::AppState;
use crate::utils;
use crate::utils::rc_block::find_ah_blocks_in_rc_block;
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures::future::join_all;
use polkadot_rest_api_config::ChainType;
use scale_decode::DecodeAsType;
use serde::{Deserialize, Serialize};
use subxt::error::StorageError;

// ============================================================================
// Query Parameters
// ============================================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OnGoingReferendaQueryParams {
    /// Block height (number) or hash (0x-prefixed hex string)
    pub at: Option<String>,
    /// Use relay chain block (for Asset Hub)
    #[serde(default)]
    pub use_rc_block: bool,
}

// ============================================================================
// Response Types
// ============================================================================

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnGoingReferendaResponse {
    pub at: AtResponse,
    pub referenda: Vec<ReferendumInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}

/// Referendum info matching Sidecar's response format exactly
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferendumInfo {
    pub id: String,
    pub decision_deposit: Option<Deposit>,
    pub enactment: EnactmentInfo,
    pub submitted: String,
    pub deciding: Option<DecidingStatus>,
}

/// Enactment info matching Sidecar's format: {"after": "14400"} or {"at": "12345"}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnactmentInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Deposit {
    pub who: String,
    pub amount: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecidingStatus {
    pub since: String,
    pub confirming: Option<String>,
}

// ============================================================================
// Scale Decode Types - For direct decoding from storage
// ============================================================================

/// Referendum status enum - we only care about Ongoing variant
#[derive(Debug, DecodeAsType)]
enum ReferendumStatus {
    Ongoing(Box<OngoingDetails>),
    #[allow(dead_code)]
    Approved(u32, Option<DepositDetails>, Option<DepositDetails>),
    #[allow(dead_code)]
    Rejected(u32, Option<DepositDetails>, Option<DepositDetails>),
    #[allow(dead_code)]
    Cancelled(u32, Option<DepositDetails>, Option<DepositDetails>),
    #[allow(dead_code)]
    TimedOut(u32, Option<DepositDetails>, Option<DepositDetails>),
    #[allow(dead_code)]
    Killed(u32),
}

/// Details for ongoing referenda - extract only what we need
#[derive(Debug, DecodeAsType)]
struct OngoingDetails {
    track: u16,
    #[allow(dead_code)]
    origin: scale_value::Value<()>,
    #[allow(dead_code)]
    proposal: scale_value::Value<()>,
    enactment: EnactmentType,
    submitted: u32,
    decision_deposit: Option<DepositDetails>,
    #[allow(dead_code)]
    submission_deposit: DepositDetails,
    deciding: Option<DecidingDetails>,
    #[allow(dead_code)]
    tally: scale_value::Value<()>,
    #[allow(dead_code)]
    in_queue: bool,
    #[allow(dead_code)]
    alarm: Option<scale_value::Value<()>>,
}

/// Enactment type enum
#[derive(Debug, DecodeAsType)]
enum EnactmentType {
    After(u32),
    At(u32),
}

/// Deposit details
#[derive(Debug, DecodeAsType)]
struct DepositDetails {
    who: [u8; 32],
    amount: u128,
}

/// Deciding status details
#[derive(Debug, DecodeAsType)]
struct DecidingDetails {
    since: u32,
    confirming: Option<u32>,
}

// ============================================================================
// Main Handler
// ============================================================================

#[utoipa::path(
    get,
    path = "/v1/pallets/on-going-referenda",
    tag = "pallets",
    summary = "On-going referenda",
    description = "Returns all currently active referenda from the Referenda pallet.",
    params(
        ("at" = Option<String>, Query, description = "Block hash or number to query at"),
        ("useRcBlock" = Option<bool>, Query, description = "Treat 'at' as relay chain block identifier")
    ),
    responses(
        (status = 200, description = "Active referenda", body = Object),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn pallets_on_going_referenda(
    State(state): State<AppState>,
    Query(params): Query<OnGoingReferendaQueryParams>,
) -> Result<Response, PalletError> {
    if params.use_rc_block {
        return handle_use_rc_block(state, params).await;
    }

    // Resolve block using the common helper
    let resolved = resolve_block_for_pallet(&state.client, params.at.as_ref()).await?;

    // Fetch all referenda from storage
    let referenda = fetch_ongoing_referenda(
        &resolved.client_at_block,
        state.chain_info.ss58_prefix,
        &resolved.at.height,
    )
    .await?;

    Ok((
        StatusCode::OK,
        Json(OnGoingReferendaResponse {
            at: resolved.at,
            referenda,
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        }),
    )
        .into_response())
}

async fn handle_use_rc_block(
    state: AppState,
    params: OnGoingReferendaQueryParams,
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
        state.get_relay_chain_rpc_client().expect("checked above"),
        state.get_relay_chain_rpc().expect("checked above"),
        Some(rc_block_id),
    )
    .await?;

    let ah_blocks = find_ah_blocks_in_rc_block(&state, &rc_resolved_block).await?;

    if ah_blocks.is_empty() {
        return Ok((StatusCode::OK, Json(Vec::<OnGoingReferendaResponse>::new())).into_response());
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

        let referenda =
            fetch_ongoing_referenda(&client_at_block, state.chain_info.ss58_prefix, &at.height)
                .await?;

        results.push(OnGoingReferendaResponse {
            at,
            referenda,
            rc_block_hash: Some(rc_block_hash.clone()),
            rc_block_number: Some(rc_block_number.clone()),
            ah_timestamp,
        });
    }

    Ok((StatusCode::OK, Json(results)).into_response())
}

// ============================================================================
// Storage Fetching
// ============================================================================

/// Fetch all ongoing referenda from the Referenda pallet storage
async fn fetch_ongoing_referenda(
    client_at_block: &ClientAtBlock,
    ss58_prefix: u16,
    block_height: &str,
) -> Result<Vec<ReferendumInfo>, PalletError> {
    let mut referenda = Vec::new();

    // First, get the ReferendumCount to know how many referenda have been created
    // Use u32 as decode target - Subxt handles decoding automatically
    let count_addr = subxt::dynamic::storage::<(), u32>("Referenda", "ReferendumCount");
    let referendum_count: u32 = match client_at_block.storage().fetch(count_addr, ()).await {
        Ok(storage_val) => match storage_val.decode() {
            Ok(count) => count,
            Err(e) => {
                tracing::warn!("Failed to decode ReferendumCount: {:?}", e);
                return Err(PalletError::StorageDecodeFailed {
                    pallet: "Referenda",
                    entry: "ReferendumCount",
                });
            }
        },
        Err(e) => {
            // Match on concrete StorageError types instead of string matching
            match &e {
                StorageError::PalletNameNotFound(name) => {
                    tracing::warn!(
                        "Referenda pallet '{}' not found at block {}",
                        name,
                        block_height
                    );
                    return Err(PalletError::PalletNotAvailableAtBlock {
                        module: "api.query.referenda".to_string(),
                        block_height: block_height.to_string(),
                    });
                }
                StorageError::StorageEntryNotFound {
                    pallet_name,
                    entry_name,
                } => {
                    tracing::warn!(
                        "Storage entry '{}.{}' not found at block {}",
                        pallet_name,
                        entry_name,
                        block_height
                    );
                    return Err(PalletError::PalletNotAvailableAtBlock {
                        module: "api.query.referenda".to_string(),
                        block_height: block_height.to_string(),
                    });
                }
                _ => {
                    tracing::warn!("Failed to fetch ReferendumCount: {:?}", e);
                    return Err(PalletError::StorageFetchFailed {
                        pallet: "Referenda",
                        entry: "ReferendumCount",
                    });
                }
            }
        }
    };

    // Iterate in batches from highest ID to lowest (ongoing referenda are usually recent)
    // Use concurrent requests for better performance
    let batch_size = 50;
    let mut id = referendum_count.saturating_sub(1) as i64;

    while id >= 0 {
        let batch_start = (id - batch_size as i64 + 1).max(0) as u32;
        let batch_end = id as u32;

        // Create futures for batch fetching - decode directly to ReferendumStatus
        let futures: Vec<_> = (batch_start..=batch_end)
            .map(|ref_id| {
                let storage_addr = subxt::dynamic::storage::<_, ReferendumStatus>(
                    "Referenda",
                    "ReferendumInfoFor",
                );
                let client = client_at_block.clone();
                async move {
                    let result = client.storage().fetch(storage_addr, (ref_id,)).await;
                    let decoded: Option<ReferendumStatus> = match result {
                        Ok(val) => val.decode().ok(),
                        Err(_) => None,
                    };
                    (ref_id, decoded)
                }
            })
            .collect();

        // Execute batch concurrently
        let results = join_all(futures).await;

        for (ref_id, decoded) in results {
            let decoded = match decoded {
                Some(d) => d,
                None => continue,
            };

            // Extract ongoing referendum info using the typed struct
            if let Some((track, ongoing)) =
                extract_ongoing_from_status(decoded, ref_id, ss58_prefix)
            {
                // Filter to only include track 0 (Root) and track 1 (WhitelistedCaller)
                if track == 0 || track == 1 {
                    referenda.push(ongoing);
                }
            }
        }

        id -= batch_size as i64;
    }

    // Sort by ID in descending order to match Sidecar's ordering (highest ID first)
    referenda.sort_by(|a, b| {
        let a_id: u32 = a.id.replace(',', "").parse().unwrap_or(0);
        let b_id: u32 = b.id.replace(',', "").parse().unwrap_or(0);
        b_id.cmp(&a_id) // Descending order
    });

    Ok(referenda)
}

/// Extract ongoing referendum info from decoded ReferendumStatus
/// Returns (track, ReferendumInfo) tuple for filtering
fn extract_ongoing_from_status(
    status: ReferendumStatus,
    id: u32,
    ss58_prefix: u16,
) -> Option<(u16, ReferendumInfo)> {
    match status {
        ReferendumStatus::Ongoing(ongoing) => {
            let ongoing = *ongoing; // Unbox
            let track = ongoing.track;

            // Extract enactment in Sidecar format
            let enactment = match ongoing.enactment {
                EnactmentType::After(blocks) => EnactmentInfo {
                    after: Some(blocks.to_string()),
                    at: None,
                },
                EnactmentType::At(block) => EnactmentInfo {
                    after: None,
                    at: Some(block.to_string()),
                },
            };

            // Extract decision deposit
            let decision_deposit = ongoing.decision_deposit.map(|d| Deposit {
                who: format_account_id(&d.who, ss58_prefix),
                amount: d.amount.to_string(),
            });

            // Extract deciding status
            let deciding = ongoing.deciding.map(|d| DecidingStatus {
                since: d.since.to_string(),
                confirming: d.confirming.map(|c| c.to_string()),
            });

            // Format ID with comma like Sidecar does (e.g., "1,308" instead of "1308")
            let formatted_id = format_id_with_comma(id);

            Some((
                track,
                ReferendumInfo {
                    id: formatted_id,
                    decision_deposit,
                    enactment,
                    submitted: ongoing.submitted.to_string(),
                    deciding,
                },
            ))
        }
        _ => None, // Not ongoing, skip
    }
}

/// Format ID with comma separator like Sidecar (e.g., 1308 -> "1,308")
fn format_id_with_comma(id: u32) -> String {
    let s = id.to_string();
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(*c);
    }
    result
}

// ============================================================================
// RC (Relay Chain) Handler
// ============================================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RcOnGoingReferendaQueryParams {
    pub at: Option<String>,
}

/// Handler for GET `/rc/pallets/on-going-referenda`
///
/// Returns ongoing referenda from the relay chain.
#[utoipa::path(
    get,
    path = "/v1/rc/pallets/on-going-referenda",
    tag = "rc",
    summary = "RC on-going referenda",
    description = "Returns all currently active referenda from the relay chain's Referenda pallet.",
    params(
        ("at" = Option<String>, Query, description = "Block hash or number to query at")
    ),
    responses(
        (status = 200, description = "Active referenda from relay chain", body = Object),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn rc_pallets_on_going_referenda(
    State(state): State<AppState>,
    Query(params): Query<RcOnGoingReferendaQueryParams>,
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

    let block_id = params
        .at
        .as_ref()
        .map(|s| s.parse::<utils::BlockId>())
        .transpose()?;
    let resolved = utils::resolve_block_with_rpc(relay_rpc_client, relay_rpc, block_id).await?;

    let client_at_block = relay_client.at_block(resolved.number).await?;

    let at = AtResponse {
        hash: resolved.hash.clone(),
        height: resolved.number.to_string(),
    };

    let referenda =
        fetch_ongoing_referenda(&client_at_block, relay_chain_info.ss58_prefix, &at.height).await?;

    Ok((
        StatusCode::OK,
        Json(OnGoingReferendaResponse {
            at,
            referenda,
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        }),
    )
        .into_response())
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // format_id_with_comma tests
    // ========================================================================

    #[test]
    fn test_format_id_with_comma_single_digit() {
        assert_eq!(format_id_with_comma(1), "1");
        assert_eq!(format_id_with_comma(9), "9");
    }

    #[test]
    fn test_format_id_with_comma_double_digit() {
        assert_eq!(format_id_with_comma(10), "10");
        assert_eq!(format_id_with_comma(99), "99");
    }

    #[test]
    fn test_format_id_with_comma_triple_digit() {
        assert_eq!(format_id_with_comma(100), "100");
        assert_eq!(format_id_with_comma(999), "999");
    }

    #[test]
    fn test_format_id_with_comma_four_digits() {
        assert_eq!(format_id_with_comma(1000), "1,000");
        assert_eq!(format_id_with_comma(1308), "1,308");
        assert_eq!(format_id_with_comma(1339), "1,339");
        assert_eq!(format_id_with_comma(1349), "1,349");
        assert_eq!(format_id_with_comma(9999), "9,999");
    }

    #[test]
    fn test_format_id_with_comma_large_numbers() {
        assert_eq!(format_id_with_comma(10000), "10,000");
        assert_eq!(format_id_with_comma(100000), "100,000");
        assert_eq!(format_id_with_comma(1000000), "1,000,000");
        assert_eq!(format_id_with_comma(1234567), "1,234,567");
    }

    #[test]
    fn test_format_id_with_comma_zero() {
        assert_eq!(format_id_with_comma(0), "0");
    }

    // ========================================================================
    // Response serialization tests
    // ========================================================================

    #[test]
    fn test_referendum_info_serialization() {
        let referendum = ReferendumInfo {
            id: "1,308".to_string(),
            decision_deposit: Some(Deposit {
                who: "13sDzot2hwoEAzXJiNe3cBiMEq19XRqrS3DMAxt9jiSNKMkA".to_string(),
                amount: "1000000000000000".to_string(),
            }),
            enactment: EnactmentInfo {
                after: Some("14400".to_string()),
                at: None,
            },
            submitted: "23496576".to_string(),
            deciding: Some(DecidingStatus {
                since: "23687165".to_string(),
                confirming: None,
            }),
        };

        let json = serde_json::to_value(&referendum).unwrap();
        assert_eq!(json["id"], "1,308");
        assert_eq!(json["decisionDeposit"]["amount"], "1000000000000000");
        assert_eq!(json["enactment"]["after"], "14400");
        assert!(json["enactment"].get("at").is_none());
        assert_eq!(json["submitted"], "23496576");
        assert_eq!(json["deciding"]["since"], "23687165");
    }

    #[test]
    fn test_referendum_info_null_fields() {
        let referendum = ReferendumInfo {
            id: "1,349".to_string(),
            decision_deposit: None,
            enactment: EnactmentInfo {
                after: Some("100".to_string()),
                at: None,
            },
            submitted: "23810220".to_string(),
            deciding: None,
        };

        let json = serde_json::to_value(&referendum).unwrap();
        assert_eq!(json["id"], "1,349");
        assert!(json["decisionDeposit"].is_null());
        assert!(json["deciding"].is_null());
    }

    #[test]
    fn test_enactment_at_variant() {
        let enactment = EnactmentInfo {
            after: None,
            at: Some("25000000".to_string()),
        };

        let json = serde_json::to_value(&enactment).unwrap();
        assert!(json.get("after").is_none());
        assert_eq!(json["at"], "25000000");
    }

    // ========================================================================
    // Query params tests
    // ========================================================================

    #[test]
    fn test_query_params_default() {
        let params: OnGoingReferendaQueryParams = serde_json::from_str("{}").unwrap();
        assert!(params.at.is_none());
        assert!(!params.use_rc_block);
    }

    #[test]
    fn test_query_params_with_at() {
        let params: OnGoingReferendaQueryParams =
            serde_json::from_str(r#"{"at": "24000000"}"#).unwrap();
        assert_eq!(params.at, Some("24000000".to_string()));
        assert!(!params.use_rc_block);
    }

    #[test]
    fn test_query_params_with_use_rc_block() {
        let params: OnGoingReferendaQueryParams =
            serde_json::from_str(r#"{"useRcBlock": true}"#).unwrap();
        assert!(params.at.is_none());
        assert!(params.use_rc_block);
    }
}
