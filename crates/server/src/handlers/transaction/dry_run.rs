use crate::state::AppState;
use crate::utils;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use parity_scale_codec::Encode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sp_core::crypto::Ss58Codec;
use subxt_rpcs::rpc_params;
use thiserror::Error;

/// Request body for transaction dry-run.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DryRunRequest {
    /// Hex-encoded signed extrinsic with 0x prefix.
    pub tx: Option<String>,
    /// Sender address in SS58 format.
    pub sender_address: Option<String>,
    /// Block height to execute against (optional).
    pub at: Option<String>,
}

/// Response for successful transaction dry-run.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DryRunResponse {
    pub result_type: String,
    pub result: Value,
}

/// Weight information in V2 format.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeightsV2 {
    pub ref_time: String,
    pub proof_size: String,
}

/// Error response for dry-run failures.
#[derive(Debug, Serialize)]
pub struct DryRunFailure {
    pub code: u16,
    pub error: String,
    pub transaction: String,
    pub cause: String,
    pub stack: String,
}

/// Errors that can occur during transaction dry-run.
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

    #[error("Relay chain not configured")]
    RelayChainNotConfigured { transaction: String },
}

impl IntoResponse for DryRunError {
    fn into_response(self) -> axum::response::Response {
        match self {
            DryRunError::MissingTx => {
                let cause = "Missing field `tx` on request body.".to_string();
                let body = Json(DryRunFailure {
                    code: 400,
                    error: "Failed to parse transaction.".to_string(),
                    transaction: String::new(),
                    cause: cause.clone(),
                    stack: format!("Error: {}\n    at dry_run", cause),
                });
                (StatusCode::BAD_REQUEST, body).into_response()
            }
            DryRunError::MissingSenderAddress { transaction } => {
                let cause = "Missing field `senderAddress` on request body.".to_string();
                let body = Json(DryRunFailure {
                    code: 400,
                    error: "Failed to parse transaction.".to_string(),
                    transaction,
                    cause: cause.clone(),
                    stack: format!("Error: {}\n    at dry_run", cause),
                });
                (StatusCode::BAD_REQUEST, body).into_response()
            }
            DryRunError::ParseFailed {
                transaction,
                cause,
                ref stack,
            } => {
                let body = Json(DryRunFailure {
                    code: 400,
                    error: "Failed to parse transaction.".to_string(),
                    transaction,
                    cause,
                    stack: stack.clone(),
                });
                (StatusCode::BAD_REQUEST, body).into_response()
            }
            DryRunError::DryRunFailed {
                transaction,
                cause,
                ref stack,
            } => {
                let body = Json(DryRunFailure {
                    code: 400,
                    error: "Unable to dry-run transaction".to_string(),
                    transaction,
                    cause,
                    stack: stack.clone(),
                });
                (StatusCode::BAD_REQUEST, body).into_response()
            }
            DryRunError::DryRunApiNotAvailable { transaction } => {
                let cause = "DryRunApi not found in metadata.".to_string();
                let body = Json(DryRunFailure {
                    code: 400,
                    error: "Unable to dry-run transaction".to_string(),
                    transaction,
                    cause: cause.clone(),
                    stack: format!("Error: {}\n    at dry_run", cause),
                });
                (StatusCode::BAD_REQUEST, body).into_response()
            }
            DryRunError::InvalidBlockParam { transaction, cause } => {
                let body = Json(DryRunFailure {
                    code: 400,
                    error: "Unable to dry-run transaction".to_string(),
                    transaction,
                    cause: cause.clone(),
                    stack: format!("Error: {}\n    at dry_run", cause),
                });
                (StatusCode::BAD_REQUEST, body).into_response()
            }
            DryRunError::BlockNotFound { transaction, cause } => {
                let body = Json(DryRunFailure {
                    code: 400,
                    error: "Unable to dry-run transaction".to_string(),
                    transaction,
                    cause: cause.clone(),
                    stack: format!("Error: {}\n    at dry_run", cause),
                });
                (StatusCode::BAD_REQUEST, body).into_response()
            }
            DryRunError::RelayChainNotConfigured { transaction } => {
                let cause = "Relay chain not configured".to_string();
                let body = Json(DryRunFailure {
                    code: 503,
                    error: "Unable to dry-run transaction".to_string(),
                    transaction,
                    cause: cause.clone(),
                    stack: format!("Error: {}\n    at dry_run_rc", cause),
                });
                (StatusCode::SERVICE_UNAVAILABLE, body).into_response()
            }
            DryRunError::RpcFailed {
                transaction,
                cause,
                ref stack,
            } => {
                let body = Json(DryRunFailure {
                    code: 500,
                    error: "Unable to dry-run transaction".to_string(),
                    transaction,
                    cause,
                    stack: stack.clone(),
                });
                (StatusCode::INTERNAL_SERVER_ERROR, body).into_response()
            }
        }
    }
}

/// Result type enum values.
const DISPATCH_OUTCOME: &str = "DispatchOutcome";
const DISPATCH_ERROR: &str = "DispatchError";
const TRANSACTION_VALIDITY_ERROR: &str = "TransactionValidityError";

/// Resolve block hash from optional `at` parameter.
async fn resolve_block_hash(
    state: &AppState,
    at: Option<&str>,
    transaction: &str,
) -> Result<String, DryRunError> {
    match at {
        None => {
            // Get latest finalized block hash
            let hash = state
                .legacy_rpc
                .chain_get_finalized_head()
                .await
                .map_err(|e| {
                    let cause = e.to_string();
                    DryRunError::RpcFailed {
                        transaction: transaction.to_string(),
                        cause: cause.clone(),
                        stack: format!("Error: {}\n    at resolve_block_hash", cause),
                    }
                })?;
            Ok(format!("{:#x}", hash))
        }
        Some(at_str) => {
            let block_id =
                at_str
                    .parse::<utils::BlockId>()
                    .map_err(|e| DryRunError::InvalidBlockParam {
                        transaction: transaction.to_string(),
                        cause: e.to_string(),
                    })?;
            match block_id {
                utils::BlockId::Hash(hash) => Ok(format!("{:#x}", hash)),
                utils::BlockId::Number(number) => {
                    let hash = state
                        .get_block_hash_at_number(number)
                        .await
                        .map_err(|e| {
                            let cause = e.to_string();
                            DryRunError::RpcFailed {
                                transaction: transaction.to_string(),
                                cause: cause.clone(),
                                stack: format!("Error: {}\n    at resolve_block_hash", cause),
                            }
                        })?
                        .ok_or_else(|| DryRunError::BlockNotFound {
                            transaction: transaction.to_string(),
                            cause: format!("Block at height {} not found", number),
                        })?;
                    Ok(hash)
                }
            }
        }
    }
}

/// Parse the dry-run result from the runtime API response.
fn parse_dry_run_result(result_hex: &str) -> Result<DryRunResponse, String> {
    let hex_str = result_hex.strip_prefix("0x").unwrap_or(result_hex);
    let bytes = hex::decode(hex_str).map_err(|e| format!("Failed to decode result: {}", e))?;

    if bytes.is_empty() {
        return Err("Empty result from dry-run".to_string());
    }

    // The result is a Result<CallDryRunEffects, XcmDryRunApiError>
    // Result encoding: 0x00 = Ok, 0x01 = Err

    let is_ok = bytes[0] == 0;

    if is_ok && bytes.len() > 1 {
        // Try to parse the CallDryRunEffects
        // CallDryRunEffects contains executionResult which is also a Result
        let inner_bytes = &bytes[1..];

        if !inner_bytes.is_empty() {
            let execution_is_ok = inner_bytes[0] == 0;

            if execution_is_ok {
                // DispatchOutcome - parse PostDispatchInfo
                // PostDispatchInfo: { actual_weight: Option<Weight>, pays_fee: Pays }
                let result_bytes = &inner_bytes[1..];

                // Try to decode PostDispatchInfo
                if let Ok((actual_weight, pays_fee)) = decode_post_dispatch_info(result_bytes) {
                    return Ok(DryRunResponse {
                        result_type: DISPATCH_OUTCOME.to_string(),
                        result: json!({
                            "actualWeight": actual_weight,
                            "paysFee": pays_fee
                        }),
                    });
                }

                // Fallback: return raw result
                return Ok(DryRunResponse {
                    result_type: DISPATCH_OUTCOME.to_string(),
                    result: json!({
                        "raw": format!("0x{}", hex::encode(result_bytes))
                    }),
                });
            } else {
                // DispatchError
                let error_bytes = &inner_bytes[1..];
                return Ok(DryRunResponse {
                    result_type: DISPATCH_ERROR.to_string(),
                    result: json!({
                        "raw": format!("0x{}", hex::encode(error_bytes))
                    }),
                });
            }
        }
    } else if !is_ok && bytes.len() > 1 {
        // TransactionValidityError (XcmDryRunApiError)
        let error_bytes = &bytes[1..];
        return Ok(DryRunResponse {
            result_type: TRANSACTION_VALIDITY_ERROR.to_string(),
            result: json!({
                "raw": format!("0x{}", hex::encode(error_bytes))
            }),
        });
    }

    // Fallback
    Ok(DryRunResponse {
        result_type: DISPATCH_OUTCOME.to_string(),
        result: json!({
            "raw": format!("0x{}", hex::encode(&bytes))
        }),
    })
}

/// Decode PostDispatchInfo from SCALE bytes.
/// Returns (actual_weight, pays_fee).
fn decode_post_dispatch_info(bytes: &[u8]) -> Result<(Option<WeightsV2>, String), ()> {
    if bytes.is_empty() {
        return Err(());
    }

    let mut offset = 0;

    // actual_weight: Option<Weight>
    // Option encoding: 0x00 = None, 0x01 = Some
    let actual_weight = if bytes[offset] == 0 {
        offset += 1;
        None
    } else if bytes[offset] == 1 && bytes.len() > offset + 1 {
        offset += 1;
        // Weight is { ref_time: Compact<u64>, proof_size: Compact<u64> }
        if let Ok((ref_time, new_offset)) = decode_compact_u64(&bytes[offset..]) {
            offset += new_offset;
            if let Ok((proof_size, new_offset)) = decode_compact_u64(&bytes[offset..]) {
                offset += new_offset;
                Some(WeightsV2 {
                    ref_time: ref_time.to_string(),
                    proof_size: proof_size.to_string(),
                })
            } else {
                return Err(());
            }
        } else {
            return Err(());
        }
    } else {
        return Err(());
    };

    // pays_fee: Pays (enum: Yes = 0, No = 1)
    let pays_fee = if offset < bytes.len() {
        match bytes[offset] {
            0 => "Yes".to_string(),
            1 => "No".to_string(),
            _ => "Unknown".to_string(),
        }
    } else {
        "Yes".to_string() // Default
    };

    Ok((actual_weight, pays_fee))
}

/// Decode a compact-encoded u64.
/// Returns (value, bytes_consumed).
fn decode_compact_u64(bytes: &[u8]) -> Result<(u64, usize), ()> {
    if bytes.is_empty() {
        return Err(());
    }

    let mode = bytes[0] & 0b11;
    match mode {
        0b00 => {
            // Single byte mode
            Ok(((bytes[0] >> 2) as u64, 1))
        }
        0b01 => {
            // Two byte mode
            if bytes.len() < 2 {
                return Err(());
            }
            let value = u16::from_le_bytes([bytes[0], bytes[1]]) >> 2;
            Ok((value as u64, 2))
        }
        0b10 => {
            // Four byte mode
            if bytes.len() < 4 {
                return Err(());
            }
            let value = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) >> 2;
            Ok((value as u64, 4))
        }
        0b11 => {
            // Big integer mode
            let byte_len = ((bytes[0] >> 2) + 4) as usize;
            if bytes.len() < 1 + byte_len || byte_len > 8 {
                return Err(());
            }
            let mut value_bytes = [0u8; 8];
            value_bytes[..byte_len].copy_from_slice(&bytes[1..1 + byte_len]);
            Ok((u64::from_le_bytes(value_bytes), 1 + byte_len))
        }
        _ => Err(()),
    }
}

/// Decode SS58 address to account bytes.
fn decode_ss58_address(address: &str) -> Result<[u8; 32], String> {
    if address.starts_with("0x") {
        // Hex format
        let hex_str = address.strip_prefix("0x").unwrap_or(address);
        let bytes = hex::decode(hex_str).map_err(|e| format!("Invalid hex: {}", e))?;
        if bytes.len() != 32 {
            return Err(format!(
                "Invalid address length: expected 32 bytes, got {}",
                bytes.len()
            ));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(arr)
    } else {
        // SS58 format
        sp_core::crypto::AccountId32::from_ss58check(address)
            .map(|a| a.into())
            .map_err(|e| format!("Invalid SS58 address: {:?}", e))
    }
}

/// Dry-run a signed extrinsic.
pub async fn dry_run(
    State(state): State<AppState>,
    Json(body): Json<DryRunRequest>,
) -> Result<Json<DryRunResponse>, DryRunError> {
    // Validate tx field
    let tx = body.tx.as_ref().ok_or(DryRunError::MissingTx)?;
    if tx.is_empty() {
        return Err(DryRunError::MissingTx);
    }

    // Validate senderAddress field
    let sender_address =
        body.sender_address
            .as_ref()
            .ok_or_else(|| DryRunError::MissingSenderAddress {
                transaction: tx.clone(),
            })?;
    if sender_address.is_empty() {
        return Err(DryRunError::MissingSenderAddress {
            transaction: tx.clone(),
        });
    }

    // Resolve block hash
    let block_hash = resolve_block_hash(&state, body.at.as_deref(), tx).await?;

    // Decode the transaction
    let tx_hex = tx.strip_prefix("0x").unwrap_or(tx);
    let tx_bytes = hex::decode(tx_hex).map_err(|e| DryRunError::ParseFailed {
        transaction: tx.clone(),
        cause: format!("Invalid hex encoding: {}", e),
        stack: "Failed to decode transaction hex".to_string(),
    })?;

    // Decode the sender address to bytes
    let sender_bytes =
        decode_ss58_address(sender_address).map_err(|e| DryRunError::ParseFailed {
            transaction: tx.clone(),
            cause: e,
            stack: "Failed to decode sender address".to_string(),
        })?;

    // Encode parameters for DryRunApi_dry_run_call
    // The API expects: (origin: OriginCaller, call: RuntimeCall)
    // Origin: RuntimeOrigin::signed(account)

    // Build SCALE-encoded parameters
    let mut params = Vec::new();

    // Origin caller: system::RawOrigin::Signed(account)
    // Encoded as: pallet variant (0 for system) + RawOrigin variant (0 for Signed typically) + account
    params.push(0u8); // System pallet variant
    params.push(0u8); // Signed variant
    params.extend_from_slice(&sender_bytes);

    // Call: the extrinsic bytes with length prefix
    let tx_len = tx_bytes.len() as u32;
    params.extend(tx_len.encode());
    params.extend_from_slice(&tx_bytes);

    let params_hex = format!("0x{}", hex::encode(&params));

    // Call the DryRunApi_dry_run_call runtime API
    let result: Result<String, _> = state
        .rpc_client
        .request(
            "state_call",
            rpc_params!["DryRunApi_dry_run_call", &params_hex, &block_hash],
        )
        .await;

    match result {
        Ok(result_hex) => {
            // Parse the result
            match parse_dry_run_result(&result_hex) {
                Ok(response) => Ok(Json(response)),
                Err(e) => Err(DryRunError::DryRunFailed {
                    transaction: tx.clone(),
                    cause: e,
                    stack: "Failed to parse dry-run result".to_string(),
                }),
            }
        }
        Err(e) => {
            let error_str = e.to_string();

            // Check if DryRunApi is not available
            if error_str.contains("not found") || error_str.contains("does not exist") {
                return Err(DryRunError::DryRunApiNotAvailable {
                    transaction: tx.clone(),
                });
            }

            Err(DryRunError::DryRunFailed {
                transaction: tx.clone(),
                cause: error_str.clone(),
                stack: format!("Error: {}\n    at dry_run", error_str),
            })
        }
    }
}

/// Dry-run a signed extrinsic on the relay chain.
pub async fn dry_run_rc(
    State(state): State<AppState>,
    Json(body): Json<DryRunRequest>,
) -> Result<Json<DryRunResponse>, DryRunError> {
    // Validate tx field
    let tx = body.tx.as_ref().ok_or(DryRunError::MissingTx)?;
    if tx.is_empty() {
        return Err(DryRunError::MissingTx);
    }

    // Validate senderAddress field
    let sender_address =
        body.sender_address
            .as_ref()
            .ok_or_else(|| DryRunError::MissingSenderAddress {
                transaction: tx.clone(),
            })?;
    if sender_address.is_empty() {
        return Err(DryRunError::MissingSenderAddress {
            transaction: tx.clone(),
        });
    }

    // Get relay chain RPC client
    let relay_rpc =
        state
            .get_relay_chain_rpc_client()
            .ok_or_else(|| DryRunError::RelayChainNotConfigured {
                transaction: tx.clone(),
            })?;

    // Resolve block hash using relay chain
    let block_hash = match &body.at {
        None => {
            let relay_legacy = state.get_relay_chain_rpc().ok_or_else(|| {
                DryRunError::RelayChainNotConfigured {
                    transaction: tx.clone(),
                }
            })?;
            let hash = relay_legacy.chain_get_finalized_head().await.map_err(|e| {
                let cause = e.to_string();
                DryRunError::RpcFailed {
                    transaction: tx.clone(),
                    cause: cause.clone(),
                    stack: format!("Error: {}\n    at dry_run_rc", cause),
                }
            })?;
            format!("{:#x}", hash)
        }
        Some(at_str) => {
            let block_id =
                at_str
                    .parse::<utils::BlockId>()
                    .map_err(|e| DryRunError::InvalidBlockParam {
                        transaction: tx.clone(),
                        cause: e.to_string(),
                    })?;
            match block_id {
                utils::BlockId::Hash(hash) => format!("{:#x}", hash),
                utils::BlockId::Number(number) => {
                    let hash: Option<String> = relay_rpc
                        .request("chain_getBlockHash", rpc_params![number])
                        .await
                        .map_err(|e| {
                            let cause = e.to_string();
                            DryRunError::RpcFailed {
                                transaction: tx.clone(),
                                cause: cause.clone(),
                                stack: format!("Error: {}\n    at dry_run_rc", cause),
                            }
                        })?;
                    hash.ok_or_else(|| DryRunError::BlockNotFound {
                        transaction: tx.clone(),
                        cause: format!("Block at height {} not found", number),
                    })?
                }
            }
        }
    };

    // Decode the transaction and sender
    let tx_hex = tx.strip_prefix("0x").unwrap_or(tx);
    let tx_bytes = hex::decode(tx_hex).map_err(|e| DryRunError::ParseFailed {
        transaction: tx.clone(),
        cause: format!("Invalid hex encoding: {}", e),
        stack: "Failed to decode transaction hex".to_string(),
    })?;

    let sender_bytes =
        decode_ss58_address(sender_address).map_err(|e| DryRunError::ParseFailed {
            transaction: tx.clone(),
            cause: e,
            stack: "Failed to decode sender address".to_string(),
        })?;

    // Build params
    let mut params = Vec::new();
    params.push(0u8); // System pallet
    params.push(0u8); // Signed variant
    params.extend_from_slice(&sender_bytes);
    let tx_len = tx_bytes.len() as u32;
    params.extend(tx_len.encode());
    params.extend_from_slice(&tx_bytes);

    let params_hex = format!("0x{}", hex::encode(&params));

    // Call via relay chain
    let result: Result<String, _> = relay_rpc
        .request(
            "state_call",
            rpc_params!["DryRunApi_dry_run_call", &params_hex, &block_hash],
        )
        .await;

    match result {
        Ok(result_hex) => match parse_dry_run_result(&result_hex) {
            Ok(response) => Ok(Json(response)),
            Err(e) => Err(DryRunError::DryRunFailed {
                transaction: tx.clone(),
                cause: e,
                stack: "Failed to parse dry-run result".to_string(),
            }),
        },
        Err(e) => {
            let error_str = e.to_string();
            if error_str.contains("not found") || error_str.contains("does not exist") {
                return Err(DryRunError::DryRunApiNotAvailable {
                    transaction: tx.clone(),
                });
            }
            Err(DryRunError::DryRunFailed {
                transaction: tx.clone(),
                cause: error_str.clone(),
                stack: format!("Error: {}\n    at dry_run_rc", error_str),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_compact_u64_single_byte() {
        // Value 0 -> 0b00000000
        let (value, len) = decode_compact_u64(&[0b00000000]).unwrap();
        assert_eq!(value, 0);
        assert_eq!(len, 1);

        // Value 1 -> 0b00000100
        let (value, len) = decode_compact_u64(&[0b00000100]).unwrap();
        assert_eq!(value, 1);
        assert_eq!(len, 1);

        // Value 63 -> 0b11111100
        let (value, len) = decode_compact_u64(&[0b11111100]).unwrap();
        assert_eq!(value, 63);
        assert_eq!(len, 1);
    }

    #[test]
    fn test_dry_run_response_serialization() {
        let response = DryRunResponse {
            result_type: "DispatchOutcome".to_string(),
            result: json!({
                "actualWeight": {
                    "refTime": "1000000",
                    "proofSize": "2000"
                },
                "paysFee": "Yes"
            }),
        };
        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["resultType"], "DispatchOutcome");
        assert_eq!(json["result"]["actualWeight"]["refTime"], "1000000");
        assert_eq!(json["result"]["paysFee"], "Yes");
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
        assert_eq!(json["error"], "Unable to dry-run transaction");
        assert_eq!(json["transaction"], "0x1234");
        assert_eq!(json["cause"], "DryRunApi not found");
        assert!(json["stack"].as_str().unwrap().contains("Error:"));
    }

    #[test]
    fn test_decode_ss58_address() {
        // Test with Alice's address
        let result = decode_ss58_address("5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY");
        assert!(result.is_ok());
        let bytes = result.unwrap();
        assert_eq!(bytes.len(), 32);
    }
}
