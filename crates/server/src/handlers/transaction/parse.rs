// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Transaction parsing handler.
//!
//! This module provides the `/v1/transaction/parse` endpoint for decoding
//! raw transactions without executing or submitting them.

use crate::state::{AppState, RelayChainError};
use crate::utils::{self, EraInfo};
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use frame_decode::extrinsics::decode_extrinsic;
use frame_decode::helpers::decode_with_visitor;
use heck::ToLowerCamelCase;
use parity_scale_codec::Decode;
use scale_value::scale::ValueVisitor;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sp_core::crypto::{AccountId32, Ss58Codec};
use sp_runtime::traits::BlakeTwo256;
use sp_runtime::traits::Hash as HashT;
use subxt::{OnlineClient, SubstrateConfig};
use subxt_metadata::Metadata;
use thiserror::Error;

/// Request body for transaction parsing.
#[derive(Debug, Deserialize)]
pub struct ParseRequest {
    /// Hex-encoded extrinsic with optional 0x prefix.
    pub tx: Option<String>,
}

/// Response for successful transaction parsing.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseResponse {
    /// Whether the extrinsic is signed
    pub is_signed: bool,
    /// Pallet and method information
    pub method: MethodInfo,
    /// Decoded call arguments
    pub args: serde_json::Map<String, serde_json::Value>,
    /// Signature information (only present for signed extrinsics)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<SignatureInfo>,
    /// Account nonce (only present for signed extrinsics)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    /// Tip amount (only present for signed extrinsics)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tip: Option<String>,
    /// Era/mortality information
    pub era: EraInfo,
    /// Blake2-256 hash of the extrinsic
    pub hash: String,
}

/// Method information (pallet and method name).
#[derive(Debug, Serialize)]
pub struct MethodInfo {
    pub pallet: String,
    pub method: String,
}

/// Signature information for signed extrinsics.
#[derive(Debug, Serialize)]
pub struct SignatureInfo {
    pub signer: SignerId,
    pub signature: String,
}

/// Signer identifier.
#[derive(Debug, Serialize)]
pub struct SignerId {
    pub id: String,
}

/// Error response for transaction parsing failures.
#[derive(Debug, Serialize)]
pub struct ParseError {
    pub code: u16,
    pub error: String,
    pub transaction: String,
    pub cause: String,
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

/// CheckNonce signed extension - contains the account nonce
#[derive(Decode)]
struct CheckNonce(#[codec(compact)] u32);

/// ChargeTransactionPayment signed extension - contains the tip amount
#[derive(Decode)]
struct ChargeTransactionPayment(#[codec(compact)] u128);

/// ChargeAssetTxPayment signed extension - contains tip and optional asset_id
#[derive(Decode)]
struct ChargeAssetTxPayment {
    #[codec(compact)]
    tip: u128,
}

#[utoipa::path(
    post,
    path = "/v1/transaction/parse",
    tag = "transaction",
    summary = "Parse transaction",
    description = "Decode a raw transaction and return its components without executing or submitting it.",
    request_body(content = Object, description = "Transaction with 'tx' field containing hex-encoded extrinsic"),
    responses(
        (status = 200, description = "Parsed transaction", body = Object),
        (status = 400, description = "Invalid transaction"),
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
    description = "Decode a raw transaction using relay chain metadata. Only available on parachains.",
    request_body(content = Object, description = "Transaction with 'tx' field containing hex-encoded extrinsic"),
    responses(
        (status = 200, description = "Parsed transaction", body = Object),
        (status = 400, description = "Invalid transaction or relay chain not configured"),
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

    let relay_chain_info = state.get_relay_chain_info().await.map_err(|e| {
        ParseErrorKind::RelayChain {
            source: e,
            transaction: tx_str.to_string(),
        }
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
    let tx_bytes =
        hex::decode(tx.strip_prefix("0x").unwrap_or(tx)).map_err(|e| ParseErrorKind::ParseFailed {
            transaction: tx.to_string(),
            cause: format!("Invalid hex encoding: {}", e),
            stack: format!("Error: Invalid hex encoding: {}\n    at parse", e),
        })?;

    // Calculate hash
    let hash_bytes = BlakeTwo256::hash(&tx_bytes);
    let hash = format!("0x{}", hex::encode(hash_bytes.as_ref()));

    // Get metadata from current block
    let client_at_block = client.at_current_block().await.map_err(|e| {
        ParseErrorKind::ParseFailed {
            transaction: tx.to_string(),
            cause: format!("Failed to get current block: {}", e),
            stack: format!("Error: Failed to get current block: {}\n    at parse", e),
        }
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

    // Extract call arguments
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

/// Decode call arguments from extrinsic
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

        // Decode argument value using scale_value visitor
        match decode_with_visitor(
            &mut &arg_bytes[..],
            *arg.ty(),
            types,
            ValueVisitor::new(),
        ) {
            Ok(value) => {
                // Transform the scale_value to JSON with SS58 conversion
                let json_value = transform_scale_value_to_json(&value, ss58_prefix);
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

/// Transform scale_value::Value to serde_json::Value with SS58 address conversion
#[allow(clippy::only_used_in_recursion)]
fn transform_scale_value_to_json(value: &scale_value::Value<u32>, ss58_prefix: u16) -> Value {
    use scale_value::ValueDef;

    match &value.value {
        ValueDef::Composite(composite) => {
            match composite {
                scale_value::Composite::Named(fields) => {
                    let mut map = serde_json::Map::new();
                    for (name, val) in fields {
                        map.insert(name.clone(), transform_scale_value_to_json(val, ss58_prefix));
                    }
                    Value::Object(map)
                }
                scale_value::Composite::Unnamed(vals) => {
                    if vals.len() == 1 {
                        // Single unnamed field - unwrap it
                        transform_scale_value_to_json(&vals[0], ss58_prefix)
                    } else {
                        // Multiple unnamed fields - array
                        Value::Array(
                            vals.iter()
                                .map(|v| transform_scale_value_to_json(v, ss58_prefix))
                                .collect(),
                        )
                    }
                }
            }
        }
        ValueDef::Variant(variant) => {
            let inner = match &variant.values {
                scale_value::Composite::Named(fields) if fields.is_empty() => {
                    // Unit variant - just the name as string
                    return Value::String(variant.name.clone());
                }
                scale_value::Composite::Unnamed(vals) if vals.is_empty() => {
                    // Unit variant
                    return Value::String(variant.name.clone());
                }
                scale_value::Composite::Unnamed(vals) if vals.len() == 1 => {
                    // Single value variant
                    transform_scale_value_to_json(&vals[0], ss58_prefix)
                }
                scale_value::Composite::Named(fields) => {
                    let mut map = serde_json::Map::new();
                    for (name, val) in fields {
                        map.insert(name.clone(), transform_scale_value_to_json(val, ss58_prefix));
                    }
                    Value::Object(map)
                }
                scale_value::Composite::Unnamed(vals) => {
                    Value::Array(
                        vals.iter()
                            .map(|v| transform_scale_value_to_json(v, ss58_prefix))
                            .collect(),
                    )
                }
            };

            // For named variants like MultiAddress::Id, wrap with variant name in lower camelCase
            let variant_name = variant.name.to_lower_camel_case();
            let mut map = serde_json::Map::new();
            map.insert(variant_name, inner);
            Value::Object(map)
        }
        ValueDef::Primitive(prim) => {
            use scale_value::Primitive;
            match prim {
                Primitive::Bool(b) => Value::Bool(*b),
                Primitive::Char(c) => Value::String(c.to_string()),
                Primitive::String(s) => Value::String(s.clone()),
                Primitive::U128(n) => Value::String(n.to_string()),
                Primitive::I128(n) => Value::String(n.to_string()),
                Primitive::U256(n) => Value::String(format!("{:?}", n)),
                Primitive::I256(n) => Value::String(format!("{:?}", n)),
            }
        }
        ValueDef::BitSequence(bits) => {
            // Convert to hex
            let bytes: Vec<u8> = bits.iter().map(|b| if b { 1 } else { 0 }).collect();
            Value::String(format!("0x{}", hex::encode(bytes)))
        }
    }
}

/// Result type for signed extrinsic info extraction
type SignedInfoResult = (Option<SignatureInfo>, Option<String>, Option<String>, EraInfo);

/// Extract signature info, nonce, tip, and era from signed extrinsic
fn extract_signed_info(
    tx_bytes: &[u8],
    extrinsic: &frame_decode::extrinsics::Extrinsic<'_, u32>,
    metadata: &Metadata,
    ss58_prefix: u16,
    tx: &str,
) -> Result<SignedInfoResult, ParseErrorKind> {
    let types = metadata.types();

    // Get signature payload
    let sig_payload = extrinsic.signature_payload().ok_or_else(|| {
        ParseErrorKind::ParseFailed {
            transaction: tx.to_string(),
            cause: "Missing signature payload in signed extrinsic".to_string(),
            stack: "Error: Missing signature payload\n    at parse".to_string(),
        }
    })?;

    // Decode address
    let addr_bytes = &tx_bytes[sig_payload.address_range()];
    let signer_ss58 = decode_address_bytes(addr_bytes, sig_payload.address_type(), types, ss58_prefix);

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

    // Extract era from raw extrinsic bytes
    let era_info = utils::extract_era_from_extrinsic_bytes(tx_bytes).unwrap_or(EraInfo {
        immortal_era: Some("0x00".to_string()),
        mortal_era: None,
    });

    // Extract nonce and tip from transaction extensions
    let (nonce, tip) = if let Some(extensions) = extrinsic.transaction_extension_payload() {
        let mut nonce_value = None;
        let mut tip_value = None;

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
                _ => {}
            }
        }

        (nonce_value, tip_value)
    } else {
        (None, None)
    };

    Ok((signature_info, nonce, tip, era_info))
}

/// Decode address bytes to SS58 string
fn decode_address_bytes(
    addr_bytes: &[u8],
    address_type: &u32,
    types: &scale_info::PortableRegistry,
    ss58_prefix: u16,
) -> String {
    // Try to decode as scale_value first
    if let Ok(value) = decode_with_visitor(
        &mut &addr_bytes[..],
        *address_type,
        types,
        ValueVisitor::new(),
    ) {
        // Extract AccountId32 bytes from the value
        if let Some(account_bytes) = extract_account_id_bytes(&value) {
            let account_id = AccountId32::from(account_bytes);
            return account_id.to_ss58check_with_version(ss58_prefix.into());
        }
    }

    // Fallback: try direct interpretation
    if addr_bytes.len() >= 33 && addr_bytes[0] == 0x00 {
        // MultiAddress::Id variant
        let mut account_bytes = [0u8; 32];
        account_bytes.copy_from_slice(&addr_bytes[1..33]);
        let account_id = AccountId32::from(account_bytes);
        return account_id.to_ss58check_with_version(ss58_prefix.into());
    }

    // Last resort: hex representation
    format!("0x{}", hex::encode(addr_bytes))
}

/// Extract AccountId32 bytes from a scale_value::Value
fn extract_account_id_bytes(value: &scale_value::Value<u32>) -> Option<[u8; 32]> {
    use scale_value::{Composite, ValueDef};

    match &value.value {
        // MultiAddress::Id(bytes)
        ValueDef::Variant(variant) if variant.name == "Id" => {
            if let Composite::Unnamed(vals) = &variant.values
                && let Some(inner) = vals.first()
            {
                return extract_account_id_bytes(inner);
            }
        }
        // AccountId32 as 32 bytes composite
        ValueDef::Composite(Composite::Unnamed(vals)) if vals.len() == 32 => {
            let mut bytes = [0u8; 32];
            for (i, val) in vals.iter().enumerate() {
                if let ValueDef::Primitive(scale_value::Primitive::U128(n)) = &val.value {
                    bytes[i] = *n as u8;
                } else {
                    return None;
                }
            }
            return Some(bytes);
        }
        _ => {}
    }
    None
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
