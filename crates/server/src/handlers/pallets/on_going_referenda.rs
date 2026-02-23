// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Handler for the `/pallets/on-going-referenda` endpoint.
//!
//! This endpoint returns all currently active (ongoing) referenda from the
//! Referenda pallet. Only relay chains (Polkadot, Kusama) support this endpoint
//! as parachains don't have governance.

use crate::extractors::JsonQuery;
use crate::handlers::pallets::common::{
    AtResponse, ClientAtBlock, PalletError, format_account_id, resolve_block_for_pallet,
};
use crate::handlers::common::xcm_types::format_number_with_commas;
use crate::handlers::runtime_queries::governance as governance_queries;
use crate::handlers::runtime_queries::referenda as referenda_queries;
use crate::state::AppState;
use crate::utils;
use crate::utils::rc_block::find_ah_blocks_in_rc_block;
use crate::utils::run_with_concurrency;
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use polkadot_rest_api_config::ChainType;
use serde::{Deserialize, Serialize};

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
// Main Handler
// ============================================================================

#[utoipa::path(
    get,
    path = "/v1/pallets/on-going-referenda",
    tag = "pallets",
    summary = "On-going referenda",
    description = "Returns all currently active referenda from the Referenda pallet.",
    params(
        ("at" = Option<String>, description = "Block hash or number to query at"),
        ("useRcBlock" = Option<bool>, description = "Treat 'at' as relay chain block identifier")
    ),
    responses(
        (status = 200, description = "Active referenda", body = Object),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn pallets_on_going_referenda(
    State(state): State<AppState>,
    JsonQuery(params): JsonQuery<OnGoingReferendaQueryParams>,
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
    let referendum_count: u32 = match governance_queries::get_referendum_count(client_at_block).await {
        Some(count) => count,
        None => {
            // The pallet or storage entry doesn't exist at this block
            return Err(PalletError::PalletNotAvailableAtBlock {
                module: "api.query.referenda".to_string(),
                block_height: block_height.to_string(),
            });
        }
    };

    // Iterate in batches from highest ID to lowest (ongoing referenda are usually recent)
    // Use concurrent requests for better performance
    let batch_size = 50;
    let mut id = referendum_count.saturating_sub(1) as i64;

    while id >= 0 {
        let batch_start = (id - batch_size as i64 + 1).max(0) as u32;
        let batch_end = id as u32;

        // Fetch batch using centralized query
        let results = referenda_queries::iter_referenda_batch(
            client_at_block,
            batch_start,
            batch_end,
        )
        .await;

        for (ref_id, decoded) in results {
            let decoded = match decoded {
                Some(d) => d,
                None => continue,
            };
            (ref_id, decoded)
        }
    });

            // Extract ongoing referendum info using the centralized function
            if let Some((track, ongoing)) =
                referenda_queries::extract_ongoing_referendum(decoded, ref_id)
            {
                // Filter to only include track 0 (Root) and track 1 (WhitelistedCaller)
                if track == 0 || track == 1 {
                    referenda.push(convert_to_referendum_info(ongoing, ss58_prefix));
                }
            }
        }
    }

    // Sort by ID in descending order to match Sidecar's ordering (highest ID first)
    referenda.sort_by(|a, b| {
        let a_id: u32 = a.id.replace(',', "").parse().unwrap_or(0);
        let b_id: u32 = b.id.replace(',', "").parse().unwrap_or(0);
        b_id.cmp(&a_id) // Descending order
    });

    Ok(referenda)
}

/// Convert decoded ongoing referendum to handler's ReferendumInfo format
fn convert_to_referendum_info(
    decoded: referenda_queries::DecodedOngoingReferendum,
    ss58_prefix: u16,
) -> ReferendumInfo {
    use referenda_queries::DecodedEnactment;

    let enactment = match decoded.enactment {
        DecodedEnactment::After(blocks) => EnactmentInfo {
            after: Some(blocks.to_string()),
            at: None,
        },
        DecodedEnactment::At(block) => EnactmentInfo {
            after: None,
            at: Some(block.to_string()),
        },
    };

    let decision_deposit = decoded.decision_deposit.map(|d| Deposit {
        who: format_account_id(&d.who, ss58_prefix),
        amount: d.amount.to_string(),
    });

    let deciding = decoded.deciding.map(|d| DecidingStatus {
        since: d.since.to_string(),
        confirming: d.confirming.map(|c| c.to_string()),
    });

    ReferendumInfo {
        id: format_number_with_commas(decoded.id as u128),
        decision_deposit,
        enactment,
        submitted: decoded.submitted.to_string(),
        deciding,
    }
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
        ("at" = Option<String>, description = "Block hash or number to query at")
    ),
    responses(
        (status = 200, description = "Active referenda from relay chain", body = Object),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn rc_pallets_on_going_referenda(
    State(state): State<AppState>,
    JsonQuery(params): JsonQuery<RcOnGoingReferendaQueryParams>,
) -> Result<Response, PalletError> {
    let relay_client = state.get_relay_chain_client().await?;
    let relay_rpc_client = state.get_relay_chain_rpc_client().await?;
    let relay_rpc = state.get_relay_chain_rpc().await?;
    let relay_chain_info = state.get_relay_chain_info().await?;

    let block_id = params
        .at
        .as_ref()
        .map(|s| s.parse::<utils::BlockId>())
        .transpose()?;
    let resolved = utils::resolve_block_with_rpc(&relay_rpc_client, &relay_rpc, block_id).await?;

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
    // format_number_with_commas tests (centralized function)
    // ========================================================================

    #[test]
    fn test_format_number_with_commas_single_digit() {
        assert_eq!(format_number_with_commas(1), "1");
        assert_eq!(format_number_with_commas(9), "9");
    }

    #[test]
    fn test_format_number_with_commas_double_digit() {
        assert_eq!(format_number_with_commas(10), "10");
        assert_eq!(format_number_with_commas(99), "99");
    }

    #[test]
    fn test_format_number_with_commas_triple_digit() {
        assert_eq!(format_number_with_commas(100), "100");
        assert_eq!(format_number_with_commas(999), "999");
    }

    #[test]
    fn test_format_number_with_commas_four_digits() {
        assert_eq!(format_number_with_commas(1000), "1,000");
        assert_eq!(format_number_with_commas(1308), "1,308");
        assert_eq!(format_number_with_commas(1339), "1,339");
        assert_eq!(format_number_with_commas(1349), "1,349");
        assert_eq!(format_number_with_commas(9999), "9,999");
    }

    #[test]
    fn test_format_number_with_commas_large_numbers() {
        assert_eq!(format_number_with_commas(10000), "10,000");
        assert_eq!(format_number_with_commas(100000), "100,000");
        assert_eq!(format_number_with_commas(1000000), "1,000,000");
        assert_eq!(format_number_with_commas(1234567), "1,234,567");
    }

    #[test]
    fn test_format_number_with_commas_zero() {
        assert_eq!(format_number_with_commas(0), "0");
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

    #[test]
    fn test_on_going_referenda_query_params_rejects_unknown_fields() {
        let json = r#"{"at": "12345", "unknownField": true}"#;
        let result: Result<OnGoingReferendaQueryParams, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }

    #[test]
    fn test_rc_on_going_referenda_query_params_rejects_unknown_fields() {
        let json = r#"{"at": "12345", "unknownField": true}"#;
        let result: Result<RcOnGoingReferendaQueryParams, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }
}
