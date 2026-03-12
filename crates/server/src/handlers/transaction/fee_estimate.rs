// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::state::{AppState, RelayChainError};
use crate::utils::{WeightRaw, decode_runtime_dispatch_info};
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use parity_scale_codec::Encode;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use utoipa::ToSchema;

/// NOTE: Refrain from using DecodeAsType here, to avoid issues such as the one
/// fixed by https://github.com/paritytech/polkadot-rest-api/pull/292.

#[derive(Debug, Deserialize)]
pub struct FeeEstimateRequest {
    pub tx: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct FeeEstimateResponse {
    pub weight: Weight,
    pub class: String,
    pub partial_fee: String,
}

#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct Weight {
    pub ref_time: String,
    pub proof_size: String,
}

#[derive(Debug, Serialize)]
pub struct At {
    pub hash: String,
}

#[derive(Debug, Serialize)]
pub struct FeeEstimateFailure {
    pub code: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<At>,
    pub error: String,
    pub transaction: String,
    pub cause: String,
    pub stack: String,
}

#[derive(Debug, Error)]
pub enum FeeEstimateError {
    #[error("Missing field `tx` on request body.")]
    MissingTx,

    #[error("Unable to fetch fee info")]
    FetchFailed {
        at_hash: Option<String>,
        transaction: String,
        cause: String,
        stack: String,
    },

    #[error("Relay chain error")]
    RelayChain {
        source: RelayChainError,
        transaction: String,
    },
}

impl IntoResponse for FeeEstimateError {
    fn into_response(self) -> axum::response::Response {
        let (status, code, at_hash, error_msg, transaction, cause, stack) = match self {
            FeeEstimateError::MissingTx => {
                let cause = "Missing field `tx` on request body.".to_string();
                (
                    StatusCode::BAD_REQUEST,
                    400,
                    None,
                    "Unable to fetch fee info",
                    String::new(),
                    cause.clone(),
                    format!("Error: {}\n    at fee_estimate", cause),
                )
            }
            FeeEstimateError::FetchFailed {
                at_hash,
                transaction,
                cause,
                stack,
            } => (
                StatusCode::BAD_REQUEST,
                400,
                at_hash,
                "Unable to fetch fee info",
                transaction,
                cause,
                stack,
            ),
            FeeEstimateError::RelayChain {
                source,
                transaction,
            } => {
                let status = match source {
                    RelayChainError::NotConfigured => StatusCode::BAD_REQUEST,
                    RelayChainError::ConnectionFailed(_) => StatusCode::SERVICE_UNAVAILABLE,
                };
                let cause = source.to_string();
                (
                    status,
                    status.as_u16(),
                    None,
                    "Unable to fetch fee info",
                    transaction,
                    cause.clone(),
                    format!("Error: {}\n    at fee_estimate", cause),
                )
            }
        };

        let body = Json(FeeEstimateFailure {
            code,
            at: at_hash.map(|hash| At { hash }),
            error: error_msg.to_string(),
            transaction,
            cause,
            stack,
        });
        (status, body).into_response()
    }
}

#[utoipa::path(
    post,
    path = "/v1/transaction/fee-estimate",
    tag = "transaction",
    summary = "Estimate transaction fee",
    description = "Estimate the fee for a transaction.",
    request_body(content = Object, description = "Transaction with 'tx' field containing hex-encoded transaction"),
    responses(
        (status = 200, description = "Fee estimate", body = FeeEstimateResponse),
        (status = 400, description = "Invalid transaction"),
        (status = 503, description = "Service unavailable"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn fee_estimate(
    State(state): State<AppState>,
    Json(body): Json<FeeEstimateRequest>,
) -> Result<Json<FeeEstimateResponse>, FeeEstimateError> {
    fee_estimate_internal(&state.client, body).await
}

#[utoipa::path(
    post,
    path = "/v1/rc/transaction/fee-estimate",
    tag = "rc",
    summary = "RC fee estimate",
    description = "Estimate the fee for a relay chain transaction.",
    request_body(content = Object, description = "Transaction with 'tx' field"),
    responses(
        (status = 200, description = "Fee estimate", body = FeeEstimateResponse),
        (status = 400, description = "Invalid transaction"),
        (status = 503, description = "Service unavailable"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn fee_estimate_rc(
    State(state): State<AppState>,
    Json(body): Json<FeeEstimateRequest>,
) -> Result<Json<FeeEstimateResponse>, FeeEstimateError> {
    let tx = body.tx.as_deref().unwrap_or_default();
    let relay_client =
        state
            .get_relay_chain_client()
            .await
            .map_err(|e| FeeEstimateError::RelayChain {
                source: e,
                transaction: tx.to_string(),
            })?;

    fee_estimate_internal(&relay_client, body).await
}

async fn fee_estimate_internal(
    client: &subxt::OnlineClient<subxt::SubstrateConfig>,
    body: FeeEstimateRequest,
) -> Result<Json<FeeEstimateResponse>, FeeEstimateError> {
    let tx = body.tx.as_ref().ok_or(FeeEstimateError::MissingTx)?;
    if tx.is_empty() {
        return Err(FeeEstimateError::MissingTx);
    }

    let client_at = client.at_current_block().await.map_err(|e| {
        let cause = e.to_string();
        FeeEstimateError::FetchFailed {
            at_hash: None,
            transaction: tx.to_string(),
            cause: cause.clone(),
            stack: format!("Error: {}\n    at fee_estimate", cause),
        }
    })?;

    let block_hash = format!("{:#}", client_at.block_ref().hash());

    // Decode transaction bytes
    let tx_bytes = hex::decode(tx.strip_prefix("0x").unwrap_or(tx)).map_err(|e| {
        FeeEstimateError::FetchFailed {
            at_hash: Some(block_hash.clone()),
            transaction: tx.to_string(),
            cause: format!("Invalid hex encoding: {}", e),
            stack: format!("Error: Invalid hex encoding: {}\n    at fee_estimate", e),
        }
    })?;

    let mut params = tx_bytes.to_vec();
    let len = tx_bytes.len() as u32;
    len.encode_to(&mut params);

    let result_bytes = client_at
        .runtime_apis()
        .call_raw("TransactionPaymentApi_query_info", Some(&params))
        .await
        .map_err(|e| {
            let cause = e.to_string();
            FeeEstimateError::FetchFailed {
                at_hash: Some(block_hash.clone()),
                transaction: tx.to_string(),
                cause: cause.clone(),
                stack: format!("Error: {}\n    at fee_estimate", cause),
            }
        })?;

    let dispatch_info = decode_runtime_dispatch_info(&result_bytes).ok_or_else(|| {
        FeeEstimateError::FetchFailed {
            at_hash: Some(block_hash.clone()),
            transaction: tx.to_string(),
            cause: "Failed to decode RuntimeDispatchInfo".to_string(),
            stack: "Error: Failed to decode RuntimeDispatchInfo\n    at fee_estimate".to_string(),
        }
    })?;

    let weight = match &dispatch_info.weight {
        WeightRaw::V1(w) => Weight {
            ref_time: w.to_string(),
            proof_size: "0".to_string(),
        },
        WeightRaw::V2 {
            ref_time,
            proof_size,
        } => Weight {
            ref_time: ref_time.to_string(),
            proof_size: proof_size.to_string(),
        },
    };

    Ok(Json(FeeEstimateResponse {
        weight,
        class: dispatch_info.class,
        partial_fee: dispatch_info.partial_fee.to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fee_estimate_response_serialization() {
        let response = FeeEstimateResponse {
            weight: Weight {
                ref_time: "1000000".to_string(),
                proof_size: "2000".to_string(),
            },
            class: "Normal".to_string(),
            partial_fee: "123456789".to_string(),
        };
        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["weight"]["refTime"], "1000000");
        assert_eq!(json["weight"]["proofSize"], "2000");
        assert_eq!(json["class"], "Normal");
        assert_eq!(json["partialFee"], "123456789");
    }

    #[test]
    fn test_fee_estimate_failure_with_at_serialization() {
        let error = FeeEstimateFailure {
            code: 400,
            at: Some(At {
                hash: "0x1234567890abcdef".to_string(),
            }),
            error: "Unable to fetch fee info".to_string(),
            transaction: "0x1234".to_string(),
            cause: "Invalid transaction".to_string(),
            stack: "Error: Invalid transaction\n    at fee_estimate".to_string(),
        };
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["code"], 400);
        assert_eq!(json["at"]["hash"], "0x1234567890abcdef");
        assert_eq!(json["error"], "Unable to fetch fee info");
        assert_eq!(json["transaction"], "0x1234");
    }

    #[test]
    fn test_fee_estimate_failure_without_at_serialization() {
        let error = FeeEstimateFailure {
            code: 400,
            at: None,
            error: "Unable to fetch fee info".to_string(),
            transaction: "".to_string(),
            cause: "Missing field `tx`".to_string(),
            stack: "Error: Missing field `tx`\n    at fee_estimate".to_string(),
        };
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["code"], 400);
        assert!(json.get("at").is_none());
        assert_eq!(json["error"], "Unable to fetch fee info");
    }
}
