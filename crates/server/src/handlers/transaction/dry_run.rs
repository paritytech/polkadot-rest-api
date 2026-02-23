// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::state::{AppState, RelayChainError};
use crate::utils::BlockId;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sp_core::crypto::Ss58Codec;
use thiserror::Error;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DryRunRequest {
    pub tx: Option<String>,
    pub sender_address: Option<String>,
    pub at: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DryRunResponse {
    pub result_type: String,
    pub result: Value,
}

#[derive(Debug, Serialize)]
pub struct DryRunFailure {
    pub code: u16,
    pub error: String,
    pub transaction: String,
    pub cause: String,
    pub stack: String,
}

#[derive(Debug, Error)]
pub enum DryRunError {
    #[error("Missing field `tx` on request body.")]
    MissingTx,

    #[error("Missing field `senderAddress` on request body.")]
    MissingSenderAddress { transaction: String },

    #[error("Failed to parse transaction.")]
    ParseFailed {
        transaction: String,
        cause: String,
        stack: String,
    },

    #[error("DryRunApi not found in metadata.")]
    DryRunApiNotAvailable { transaction: String },

    #[error("Unable to dry-run transaction")]
    DryRunFailed {
        transaction: String,
        cause: String,
        stack: String,
    },

    #[error("Invalid block parameter")]
    InvalidBlockParam { transaction: String, cause: String },

    #[error("Block not found")]
    BlockNotFound { transaction: String, cause: String },

    #[error("RPC error")]
    RpcFailed {
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

impl IntoResponse for DryRunError {
    fn into_response(self) -> axum::response::Response {
        let (status, code, error_msg, transaction, cause, stack) = match self {
            DryRunError::MissingTx => {
                let cause = "Missing field `tx` on request body.".to_string();
                (
                    StatusCode::BAD_REQUEST,
                    400,
                    "Failed to parse transaction.",
                    String::new(),
                    cause.clone(),
                    format!("Error: {}\n    at dry_run", cause),
                )
            }
            DryRunError::MissingSenderAddress { transaction } => {
                let cause = "Missing field `senderAddress` on request body.".to_string();
                (
                    StatusCode::BAD_REQUEST,
                    400,
                    "Failed to parse transaction.",
                    transaction,
                    cause.clone(),
                    format!("Error: {}\n    at dry_run", cause),
                )
            }
            DryRunError::ParseFailed {
                transaction,
                cause,
                stack,
            } => (
                StatusCode::BAD_REQUEST,
                400,
                "Failed to parse transaction.",
                transaction,
                cause,
                stack,
            ),
            DryRunError::DryRunFailed {
                transaction,
                cause,
                stack,
            } => (
                StatusCode::BAD_REQUEST,
                400,
                "Unable to dry-run transaction",
                transaction,
                cause,
                stack,
            ),
            DryRunError::DryRunApiNotAvailable { transaction } => {
                let cause = "DryRunApi not found in metadata.".to_string();
                (
                    StatusCode::BAD_REQUEST,
                    400,
                    "Unable to dry-run transaction",
                    transaction,
                    cause.clone(),
                    format!("Error: {}\n    at dry_run", cause),
                )
            }
            DryRunError::InvalidBlockParam { transaction, cause }
            | DryRunError::BlockNotFound { transaction, cause } => (
                StatusCode::BAD_REQUEST,
                400,
                "Unable to dry-run transaction",
                transaction,
                cause.clone(),
                format!("Error: {}\n    at dry_run", cause),
            ),
            DryRunError::RelayChain {
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
                    "Unable to dry-run transaction",
                    transaction,
                    cause.clone(),
                    format!("Error: {}\n    at dry_run_rc", cause),
                )
            }
            DryRunError::RpcFailed {
                transaction,
                cause,
                stack,
            } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                500,
                "Unable to dry-run transaction",
                transaction,
                cause,
                stack,
            ),
        };

        let body = Json(DryRunFailure {
            code,
            error: error_msg.to_string(),
            transaction,
            cause,
            stack,
        });
        (status, body).into_response()
    }
}

// Error conversions
impl From<subxt::Error> for DryRunError {
    fn from(err: subxt::Error) -> Self {
        let cause = err.to_string();
        if cause.contains("not found") || cause.contains("does not exist") {
            DryRunError::DryRunApiNotAvailable {
                transaction: String::new(),
            }
        } else {
            DryRunError::DryRunFailed {
                transaction: String::new(),
                cause: cause.clone(),
                stack: format!("Error: {}\n    at dry_run", cause),
            }
        }
    }
}

impl From<subxt::error::OnlineClientAtBlockError> for DryRunError {
    fn from(err: subxt::error::OnlineClientAtBlockError) -> Self {
        let cause = err.to_string();
        DryRunError::BlockNotFound {
            transaction: String::new(),
            cause,
        }
    }
}

impl From<subxt::error::RuntimeApiError> for DryRunError {
    fn from(err: subxt::error::RuntimeApiError) -> Self {
        let cause = err.to_string();
        if cause.contains("not found") || cause.contains("does not exist") {
            DryRunError::DryRunApiNotAvailable {
                transaction: String::new(),
            }
        } else {
            DryRunError::DryRunFailed {
                transaction: String::new(),
                cause: cause.clone(),
                stack: format!("Error: {}\n    at dry_run", cause),
            }
        }
    }
}

#[utoipa::path(
    post,
    path = "/v1/transaction/dry-run",
    tag = "transaction",
    summary = "Dry run transaction",
    description = "Dry run a transaction to check validity without submitting.",
    request_body(content = Object, description = "Transaction with 'tx', 'senderAddress', and optional 'at' fields"),
    responses(
        (status = 200, description = "Dry run result", body = Object),
        (status = 400, description = "Invalid transaction"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn dry_run(
    State(state): State<AppState>,
    Json(body): Json<DryRunRequest>,
) -> Result<Json<DryRunResponse>, DryRunError> {
    dry_run_internal(&state.client, body).await
}

#[utoipa::path(
    post,
    path = "/v1/rc/transaction/dry-run",
    tag = "rc",
    summary = "RC dry run transaction",
    description = "Dry run a transaction on the relay chain.",
    request_body(content = Object, description = "Transaction with 'tx', 'senderAddress', and optional 'at' fields"),
    responses(
        (status = 200, description = "Dry run result", body = Object),
        (status = 400, description = "Invalid transaction"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn dry_run_rc(
    State(state): State<AppState>,
    Json(body): Json<DryRunRequest>,
) -> Result<Json<DryRunResponse>, DryRunError> {
    let tx = body.tx.as_deref().unwrap_or_default();
    let relay_client =
        state
            .get_relay_chain_client()
            .await
            .map_err(|e| DryRunError::RelayChain {
                source: e,
                transaction: tx.to_string(),
            })?;

    dry_run_internal(&relay_client, body).await
}

async fn dry_run_internal(
    client: &subxt::OnlineClient<subxt::SubstrateConfig>,
    body: DryRunRequest,
) -> Result<Json<DryRunResponse>, DryRunError> {
    let tx = body.tx.as_ref().ok_or(DryRunError::MissingTx)?;
    if tx.is_empty() {
        return Err(DryRunError::MissingTx);
    }
    let sender = validate_sender(&body.sender_address, tx)?;

    // Resolve block
    let client_at = match &body.at {
        None => client.at_current_block().await?,
        Some(at_str) => {
            let block_id =
                at_str
                    .parse::<BlockId>()
                    .map_err(|e| DryRunError::InvalidBlockParam {
                        transaction: tx.to_string(),
                        cause: e.to_string(),
                    })?;
            match block_id {
                BlockId::Hash(hash) => client.at_block(hash).await?,
                BlockId::Number(num) => client.at_block(num).await?,
            }
        }
    };

    // Build origin: { System: { Signed: account } }
    let sender_bytes = decode_ss58_address(sender).map_err(|e| DryRunError::ParseFailed {
        transaction: tx.to_string(),
        cause: e.clone(),
        stack: format!("Error: {}\n    at dry_run", e),
    })?;

    let origin = subxt::dynamic::Value::named_composite([(
        "System",
        subxt::dynamic::Value::named_composite([(
            "Signed",
            subxt::dynamic::Value::from_bytes(sender_bytes),
        )]),
    )]);

    // Decode transaction bytes
    let tx_bytes =
        hex::decode(tx.strip_prefix("0x").unwrap_or(tx)).map_err(|e| DryRunError::ParseFailed {
            transaction: tx.to_string(),
            cause: format!("Invalid hex encoding: {}", e),
            stack: format!("Error: Invalid hex encoding: {}\n    at dry_run", e),
        })?;
    let call = subxt::dynamic::Value::from_bytes(tx_bytes);

    // Call DryRunApi.dry_run_call(origin, call)
    let method = subxt::dynamic::runtime_api_call::<_, scale_value::Value<()>>(
        "DryRunApi",
        "dry_run_call",
        (origin, call),
    );
    let result = client_at.runtime_apis().call(method).await?;

    parse_result_to_response(result, tx)
}

fn validate_sender<'a>(sender: &'a Option<String>, tx: &str) -> Result<&'a str, DryRunError> {
    let sender = sender
        .as_ref()
        .ok_or_else(|| DryRunError::MissingSenderAddress {
            transaction: tx.to_string(),
        })?;
    if sender.is_empty() {
        return Err(DryRunError::MissingSenderAddress {
            transaction: tx.to_string(),
        });
    }
    Ok(sender)
}

fn decode_ss58_address(address: &str) -> Result<[u8; 32], String> {
    if address.starts_with("0x") {
        let hex_str = address.strip_prefix("0x").unwrap();
        let bytes = hex::decode(hex_str).map_err(|e| format!("Invalid hex: {}", e))?;
        if bytes.len() != 32 {
            return Err(format!(
                "Invalid address length: expected 32, got {}",
                bytes.len()
            ));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(arr)
    } else {
        sp_core::crypto::AccountId32::from_ss58check(address)
            .map(|a| a.into())
            .map_err(|e| format!("Invalid SS58 address: {:?}", e))
    }
}

fn parse_result_to_response(
    result: scale_value::Value<()>,
    _tx: &str,
) -> Result<Json<DryRunResponse>, DryRunError> {
    use scale_value::{Composite, ValueDef};

    // The result from DryRunApi is Result<CallDryRunEffects, XcmDryRunApiError>
    // Convert scale_value::Value to serde_json::Value for the response
    fn to_json(v: &scale_value::Value<()>) -> Value {
        serde_json::to_value(v).unwrap_or(Value::Null)
    }

    // Helper to get first value from a Composite
    fn get_first_value(composite: &Composite<()>) -> Option<&scale_value::Value<()>> {
        match composite {
            Composite::Unnamed(vals) => vals.first(),
            Composite::Named(vals) => vals.first().map(|(_, v)| v),
        }
    }

    // Helper to get value by key from a Composite
    fn get_named_value<'a>(
        composite: &'a Composite<()>,
        key: &str,
    ) -> Option<&'a scale_value::Value<()>> {
        match composite {
            Composite::Named(vals) => vals.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            Composite::Unnamed(_) => None,
        }
    }

    // Check if it's an Ok or Err variant
    if let ValueDef::Variant(variant) = &result.value {
        match variant.name.as_str() {
            "Ok" => {
                // Get the inner value from Ok variant
                let ok_value = get_first_value(&variant.values).unwrap_or(&result);

                // Check execution_result inside CallDryRunEffects
                if let ValueDef::Composite(composite) = &ok_value.value
                    && let Some(exec_result) = get_named_value(composite, "execution_result")
                    && let ValueDef::Variant(exec_variant) = &exec_result.value
                {
                    let inner_value = get_first_value(&exec_variant.values).unwrap_or(exec_result);

                    return match exec_variant.name.as_str() {
                        "Ok" => Ok(Json(DryRunResponse {
                            result_type: "DispatchOutcome".to_string(),
                            result: to_json(inner_value),
                        })),
                        "Err" => Ok(Json(DryRunResponse {
                            result_type: "DispatchError".to_string(),
                            result: to_json(inner_value),
                        })),
                        _ => Ok(Json(DryRunResponse {
                            result_type: "DispatchOutcome".to_string(),
                            result: to_json(ok_value),
                        })),
                    };
                };
            }
            "Err" => {
                // TransactionValidityError / XcmDryRunApiError
                let err_value = get_first_value(&variant.values).unwrap_or(&result);
                return Ok(Json(DryRunResponse {
                    result_type: "TransactionValidityError".to_string(),
                    result: to_json(err_value),
                }));
            }
            _ => {}
        }
    }

    // If the structure doesn't match expected format, return as-is
    Ok(Json(DryRunResponse {
        result_type: "DispatchOutcome".to_string(),
        result: to_json(&result),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_dry_run_response_serialization() {
        let response = DryRunResponse {
            result_type: "DispatchOutcome".to_string(),
            result: json!({ "actualWeight": { "refTime": "1000", "proofSize": "2000" }, "paysFee": "Yes" }),
        };
        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["resultType"], "DispatchOutcome");
    }

    #[test]
    fn test_dry_run_failure_serialization() {
        let error = DryRunFailure {
            code: 400,
            error: "Unable to dry-run transaction".to_string(),
            transaction: "0x1234".to_string(),
            cause: "DryRunApi not found".to_string(),
            stack: "Error: DryRunApi not found\n    at dry_run".to_string(),
        };
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["code"], 400);
        assert_eq!(json["transaction"], "0x1234");
        assert_eq!(json["cause"], "DryRunApi not found");
    }

    #[test]
    fn test_decode_ss58_address() {
        let result = decode_ss58_address("5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 32);
    }

    #[test]
    fn test_decode_ss58_address_hex() {
        let hex_addr = "0xd43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d";
        let result = decode_ss58_address(hex_addr);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 32);
    }
}
