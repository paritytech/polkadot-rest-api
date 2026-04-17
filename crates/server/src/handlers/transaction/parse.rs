// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Transaction parsing handler.
//!
//! This module provides the `/v1/transaction/parse` endpoint for decoding
//! raw transactions without executing or submitting them.

use crate::handlers::blocks::decode::args::CallArgsVisitor;
use crate::state::{AppState, RelayChainError};
use crate::utils::{
    self, ChargeAssetTxPayment, ChargeTransactionPayment, CheckNonce, EraInfo,
    decode_address_to_ss58,
};
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use frame_decode::extrinsics::decode_extrinsic;
use heck::ToLowerCamelCase;
use parity_scale_codec::Decode;
use scale_decode::visitor::decode_with_visitor;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sp_runtime::traits::BlakeTwo256;
use sp_runtime::traits::Hash as HashT;
use subxt::{OnlineClient, SubstrateConfig};
use subxt_metadata::Metadata;
use thiserror::Error;
use utoipa::ToSchema;

/// Request body for transaction parsing.
#[derive(Debug, Deserialize, ToSchema)]
pub struct ParseRequest {
    /// Hex-encoded extrinsic with optional 0x prefix.
    #[schema(example = "0x4902840004316d995f...")]
    pub tx: Option<String>,
}

/// Response for successful transaction parsing.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ParseResponse {
    /// Whether the extrinsic is signed
    #[schema(example = true)]
    pub is_signed: bool,
    /// Pallet and method information
    pub method: MethodInfo,
    /// Decoded call arguments
    #[schema(value_type = Object)]
    pub args: serde_json::Map<String, serde_json::Value>,
    /// Signature information (only present for signed extrinsics)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<SignatureInfo>,
    /// Account nonce (only present for signed extrinsics)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(example = "42")]
    pub nonce: Option<String>,
    /// Tip amount (only present for signed extrinsics)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(example = "0")]
    pub tip: Option<String>,
    /// Era/mortality information
    #[schema(value_type = Object)]
    pub era: EraInfo,
    /// Blake2-256 hash of the extrinsic
    #[schema(example = "0x1234567890abcdef...")]
    pub hash: String,
}

/// Method information (pallet and method name).
#[derive(Debug, Serialize, ToSchema)]
pub struct MethodInfo {
    /// Pallet name in lowerCamelCase
    #[schema(example = "balances")]
    pub pallet: String,
    /// Method name in lowerCamelCase
    #[schema(example = "transferAllowDeath")]
    pub method: String,
}

/// Signature information for signed extrinsics.
#[derive(Debug, Serialize, ToSchema)]
pub struct SignatureInfo {
    /// Signer account
    pub signer: SignerId,
    /// Hex-encoded signature
    #[schema(example = "0xa24152685f52e4726466e80247d965bb3d349637fc8a1ea6f7cc1451ddec98b5...")]
    pub signature: String,
}

/// Signer identifier.
#[derive(Debug, Serialize, ToSchema)]
pub struct SignerId {
    /// SS58-encoded account address
    #[schema(example = "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY")]
    pub id: String,
}

/// Error response for transaction parsing failures.
#[derive(Debug, Serialize, ToSchema)]
pub struct ParseError {
    /// HTTP status code
    #[schema(example = 400)]
    pub code: u16,
    /// Error message
    #[schema(example = "Failed to parse transaction.")]
    pub error: String,
    /// The transaction that failed to parse
    pub transaction: String,
    /// Cause of the error
    pub cause: String,
    /// Stack trace
    pub stack: String,
}

/// Errors that can occur during transaction parsing.
#[derive(Debug, Error)]
pub enum ParseErrorKind {
    #[error("Missing field `tx` on request body.")]
    MissingTx,

    #[error("Failed to parse transaction.")]
    ParseFailed {
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

impl IntoResponse for ParseErrorKind {
    fn into_response(self) -> axum::response::Response {
        match self {
            ParseErrorKind::MissingTx => {
                let cause = "Missing field `tx` on request body.".to_string();
                let body = Json(ParseError {
                    code: 400,
                    error: "Failed to parse transaction.".to_string(),
                    transaction: String::new(),
                    cause: cause.clone(),
                    stack: format!("Error: {}\n    at parse", cause),
                });
                (StatusCode::BAD_REQUEST, body).into_response()
            }
            ParseErrorKind::ParseFailed {
                transaction,
                cause,
                stack,
            } => {
                let body = Json(ParseError {
                    code: 400,
                    error: "Failed to parse transaction.".to_string(),
                    transaction,
                    cause,
                    stack,
                });
                (StatusCode::BAD_REQUEST, body).into_response()
            }
            ParseErrorKind::RelayChain {
                source,
                transaction,
            } => {
                let status = match source {
                    RelayChainError::NotConfigured => StatusCode::BAD_REQUEST,
                    RelayChainError::ConnectionFailed(_) => StatusCode::SERVICE_UNAVAILABLE,
                };
                let cause = source.to_string();
                let body = Json(ParseError {
                    code: status.as_u16(),
                    error: "Failed to parse transaction.".to_string(),
                    transaction,
                    cause: cause.clone(),
                    stack: format!("Error: {}\n    at parse", cause),
                });
                (status, body).into_response()
            }
        }
    }
}

#[utoipa::path(
    post,
    path = "/v1/transaction/parse",
    tag = "transaction",
    summary = "Parse transaction",
    description = "Decode a raw transaction and return its components without executing or submitting it. \
        Returns the decoded pallet/method, call arguments, signature info, nonce, tip, era, and hash. \
        Note: This endpoint uses the chain's current (latest) metadata for decoding. Transactions created \
        for older runtime versions may fail to decode if the extrinsic format has changed.",
    request_body(content = ParseRequest, description = "Transaction with 'tx' field containing hex-encoded extrinsic"),
    responses(
        (status = 200, description = "Parsed transaction", body = ParseResponse),
        (status = 400, description = "Invalid transaction", body = ParseError),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn parse(
    State(state): State<AppState>,
    Json(body): Json<ParseRequest>,
) -> Result<Json<ParseResponse>, ParseErrorKind> {
    parse_internal(&state.client, state.chain_info.ss58_prefix, body).await
}

#[utoipa::path(
    post,
    path = "/v1/rc/transaction/parse",
    tag = "rc",
    summary = "Parse transaction (relay chain)",
    description = "Decode a raw transaction using relay chain metadata. Only available on parachains. \
        Returns the decoded pallet/method, call arguments, signature info, nonce, tip, era, and hash. \
        Note: This endpoint uses the relay chain's current (latest) metadata for decoding. Transactions \
        created for older runtime versions may fail to decode if the extrinsic format has changed.",
    request_body(content = ParseRequest, description = "Transaction with 'tx' field containing hex-encoded extrinsic"),
    responses(
        (status = 200, description = "Parsed transaction", body = ParseResponse),
        (status = 400, description = "Invalid transaction or relay chain not configured", body = ParseError),
        (status = 503, description = "Relay chain unavailable"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn parse_rc(
    State(state): State<AppState>,
    Json(body): Json<ParseRequest>,
) -> Result<Json<ParseResponse>, ParseErrorKind> {
    let tx_str = body.tx.as_deref().unwrap_or_default();
    let relay_client =
        state
            .get_relay_chain_client()
            .await
            .map_err(|e| ParseErrorKind::RelayChain {
                source: e,
                transaction: tx_str.to_string(),
            })?;

    let relay_chain_info =
        state
            .get_relay_chain_info()
            .await
            .map_err(|e| ParseErrorKind::RelayChain {
                source: e,
                transaction: tx_str.to_string(),
            })?;

    parse_internal(&relay_client, relay_chain_info.ss58_prefix, body).await
}

async fn parse_internal(
    client: &OnlineClient<SubstrateConfig>,
    ss58_prefix: u16,
    body: ParseRequest,
) -> Result<Json<ParseResponse>, ParseErrorKind> {
    let tx = body.tx.as_ref().ok_or(ParseErrorKind::MissingTx)?;
    if tx.is_empty() {
        return Err(ParseErrorKind::MissingTx);
    }

    // Decode hex to bytes
    let tx_bytes = hex::decode(tx.strip_prefix("0x").unwrap_or(tx)).map_err(|e| {
        ParseErrorKind::ParseFailed {
            transaction: tx.to_string(),
            cause: format!("Invalid hex encoding: {}", e),
            stack: format!("Error: Invalid hex encoding: {}\n    at parse", e),
        }
    })?;

    // Calculate hash
    let hash_bytes = BlakeTwo256::hash(&tx_bytes);
    let hash = format!("0x{}", hex::encode(hash_bytes.as_ref()));

    // Get metadata from current block
    let client_at_block =
        client
            .at_current_block()
            .await
            .map_err(|e| ParseErrorKind::ParseFailed {
                transaction: tx.to_string(),
                cause: format!("Failed to get current block: {}", e),
                stack: format!("Error: Failed to get current block: {}\n    at parse", e),
            })?;

    let metadata = client_at_block.metadata();
    let types = metadata.types();

    // Decode extrinsic using frame_decode
    // Note: &*metadata dereferences Arc<Metadata> to &Metadata which implements ExtrinsicTypeInfo
    let extrinsic = decode_extrinsic(&mut &tx_bytes[..], &*metadata, types).map_err(|e| {
        ParseErrorKind::ParseFailed {
            transaction: tx.to_string(),
            cause: format!("Failed to decode extrinsic: {:?}", e),
            stack: format!("Error: Failed to decode extrinsic: {:?}\n    at parse", e),
        }
    })?;

    // Extract pallet and method name
    let pallet_name = extrinsic.pallet_name().to_lower_camel_case();
    let method_name = extrinsic.call_name().to_lower_camel_case();

    // Extract call arguments using the shared CallArgsVisitor
    let args_map = decode_call_args(&tx_bytes, &extrinsic, &metadata, ss58_prefix, tx)?;

    // Extract signature info and era
    let (signature_info, nonce, tip, era_info) = if extrinsic.is_signed() {
        extract_signed_info(&tx_bytes, &extrinsic, &metadata, ss58_prefix, tx)?
    } else {
        // Unsigned extrinsics are immortal
        (
            None,
            None,
            None,
            EraInfo {
                immortal_era: Some("0x00".to_string()),
                mortal_era: None,
            },
        )
    };

    Ok(Json(ParseResponse {
        is_signed: extrinsic.is_signed(),
        method: MethodInfo {
            pallet: pallet_name,
            method: method_name,
        },
        args: args_map,
        signature: signature_info,
        nonce,
        tip,
        era: era_info,
        hash,
    }))
}

/// Decode call arguments from extrinsic using the shared CallArgsVisitor
fn decode_call_args(
    tx_bytes: &[u8],
    extrinsic: &frame_decode::extrinsics::Extrinsic<'_, u32>,
    metadata: &Metadata,
    ss58_prefix: u16,
    tx: &str,
) -> Result<serde_json::Map<String, Value>, ParseErrorKind> {
    let types = metadata.types();
    let mut args_map = serde_json::Map::new();

    // Get call data info - it's an iterator of NamedArg
    let call_data = extrinsic.call_data();

    for arg in call_data {
        let arg_name = arg.name().to_string();
        let arg_range = arg.range();
        let arg_bytes = &tx_bytes[arg_range.clone()];

        // Use the shared CallArgsVisitor which handles:
        // - SS58 encoding for AccountId32/MultiAddress/AccountId types
        // - Preserving arrays for Vec<T> sequences
        // - Converting byte arrays to hex
        // - Basic enums as strings, non-basic enums as objects
        match decode_with_visitor(
            &mut &arg_bytes[..],
            *arg.ty(),
            types,
            CallArgsVisitor::new(ss58_prefix, types),
        ) {
            Ok(json_value) => {
                args_map.insert(arg_name, json_value);
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to decode argument '{}' in transaction {}: {:?}",
                    arg_name,
                    tx,
                    e
                );
                // Insert hex bytes as fallback
                args_map.insert(arg_name, json!(format!("0x{}", hex::encode(arg_bytes))));
            }
        }
    }

    Ok(args_map)
}

/// Result type for signed extrinsic info extraction
type SignedInfoResult = (
    Option<SignatureInfo>,
    Option<String>,
    Option<String>,
    EraInfo,
);

/// Extract signature info, nonce, tip, and era from signed extrinsic
fn extract_signed_info(
    tx_bytes: &[u8],
    extrinsic: &frame_decode::extrinsics::Extrinsic<'_, u32>,
    _metadata: &Metadata,
    ss58_prefix: u16,
    tx: &str,
) -> Result<SignedInfoResult, ParseErrorKind> {
    // Get signature payload
    let sig_payload = extrinsic
        .signature_payload()
        .ok_or_else(|| ParseErrorKind::ParseFailed {
            transaction: tx.to_string(),
            cause: "Missing signature payload in signed extrinsic".to_string(),
            stack: "Error: Missing signature payload\n    at parse".to_string(),
        })?;

    // Decode address using existing utility
    let addr_bytes = &tx_bytes[sig_payload.address_range()];
    let addr_hex = format!("0x{}", hex::encode(addr_bytes));
    let signer_ss58 = decode_address_to_ss58(&addr_hex, ss58_prefix).unwrap_or_else(|| {
        // Fallback: try direct interpretation for MultiAddress::Id variant
        if addr_bytes.len() >= 33 && addr_bytes[0] == 0x00 {
            let addr_inner_hex = format!("0x{}", hex::encode(&addr_bytes[1..33]));
            decode_address_to_ss58(&addr_inner_hex, ss58_prefix).unwrap_or(addr_hex.clone())
        } else {
            addr_hex
        }
    });

    // Get signature bytes
    let sig_bytes = &tx_bytes[sig_payload.signature_range()];
    // Strip signature type prefix if present
    let signature_hex = if sig_bytes.len() > 1 {
        format!("0x{}", hex::encode(&sig_bytes[1..]))
    } else {
        format!("0x{}", hex::encode(sig_bytes))
    };

    let signature_info = Some(SignatureInfo {
        signer: SignerId { id: signer_ss58 },
        signature: signature_hex,
    });

    // Extract nonce, tip, and era from transaction extensions using shared types
    let (nonce, tip, era_info) = if let Some(extensions) = extrinsic.transaction_extension_payload()
    {
        let mut nonce_value = None;
        let mut tip_value = None;
        let mut era_value = None;

        for ext in extensions.iter() {
            let ext_name = ext.name();
            let ext_range = ext.range();
            let ext_bytes = &tx_bytes[ext_range.clone()];

            match ext_name {
                "CheckNonce" => {
                    if let Ok(nonce) = CheckNonce::decode(&mut &ext_bytes[..]) {
                        nonce_value = Some(nonce.0.to_string());
                    }
                }
                "ChargeTransactionPayment" => {
                    if let Ok(payment) = ChargeTransactionPayment::decode(&mut &ext_bytes[..]) {
                        tip_value = Some(payment.0.to_string());
                    } else {
                        tip_value = Some("0".to_string());
                    }
                }
                "ChargeAssetTxPayment" => {
                    if let Ok(payment) = ChargeAssetTxPayment::decode(&mut &ext_bytes[..]) {
                        tip_value = Some(payment.tip.to_string());
                    } else {
                        tip_value = Some("0".to_string());
                    }
                }
                "CheckMortality" | "CheckEra" => {
                    // Decode era from the extension bytes
                    let mut offset = 0;
                    if let Some(decoded_era) = utils::decode_era_from_bytes(ext_bytes, &mut offset)
                    {
                        era_value = Some(decoded_era);
                    }
                }
                _ => {}
            }
        }

        // Use decoded era, or fallback to extracting from raw bytes, or default to immortal
        let era = era_value
            .or_else(|| utils::extract_era_from_extrinsic_bytes(tx_bytes))
            .unwrap_or(EraInfo {
                immortal_era: Some("0x00".to_string()),
                mortal_era: None,
            });

        (nonce_value, tip_value, era)
    } else {
        // No extensions - try to extract era from raw bytes
        let era = utils::extract_era_from_extrinsic_bytes(tx_bytes).unwrap_or(EraInfo {
            immortal_era: Some("0x00".to_string()),
            mortal_era: None,
        });
        (None, None, era)
    };

    Ok((signature_info, nonce, tip, era_info))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_response_serialization() {
        let response = ParseResponse {
            is_signed: true,
            method: MethodInfo {
                pallet: "balances".to_string(),
                method: "transferAllowDeath".to_string(),
            },
            args: {
                let mut map = serde_json::Map::new();
                map.insert(
                    "dest".to_string(),
                    json!({ "id": "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY" }),
                );
                map.insert("value".to_string(), json!("1000000000000"));
                map
            },
            signature: Some(SignatureInfo {
                signer: SignerId {
                    id: "5FHneW46xGXgs5mUiveU4sbTyGBzmstUspZC92UhjJM694ty".to_string(),
                },
                signature: "0x1234".to_string(),
            }),
            nonce: Some("42".to_string()),
            tip: Some("0".to_string()),
            era: EraInfo {
                immortal_era: None,
                mortal_era: Some(vec!["64".to_string(), "19".to_string()]),
            },
            hash: "0xabcd".to_string(),
        };

        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["isSigned"], true);
        assert_eq!(json["method"]["pallet"], "balances");
        assert_eq!(json["method"]["method"], "transferAllowDeath");
        assert_eq!(json["nonce"], "42");
        assert_eq!(json["tip"], "0");
    }

    #[test]
    fn test_parse_response_unsigned() {
        let response = ParseResponse {
            is_signed: false,
            method: MethodInfo {
                pallet: "timestamp".to_string(),
                method: "set".to_string(),
            },
            args: {
                let mut map = serde_json::Map::new();
                map.insert("now".to_string(), json!("1704067200000"));
                map
            },
            signature: None,
            nonce: None,
            tip: None,
            era: EraInfo {
                immortal_era: Some("0x00".to_string()),
                mortal_era: None,
            },
            hash: "0xabcd".to_string(),
        };

        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["isSigned"], false);
        assert!(json.get("signature").is_none());
        assert!(json.get("nonce").is_none());
        assert!(json.get("tip").is_none());
    }

    #[test]
    fn test_parse_error_serialization() {
        let error = ParseError {
            code: 400,
            error: "Failed to parse transaction.".to_string(),
            transaction: "0x1234".to_string(),
            cause: "Invalid extrinsic encoding".to_string(),
            stack: "Error: Invalid extrinsic encoding\n    at parse".to_string(),
        };

        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["code"], 400);
        assert_eq!(json["error"], "Failed to parse transaction.");
        assert_eq!(json["transaction"], "0x1234");
    }
}
