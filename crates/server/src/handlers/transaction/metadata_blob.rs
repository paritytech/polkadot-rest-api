// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Handler for POST /transaction/metadata-blob
//!
//! Generates a minimal metadata proof for a specific extrinsic, implementing
//! RFC-0078: https://polkadot-fellows.github.io/RFCs/approved/0078-merkleized-metadata.html.
//! This allows offline signers to decode transactions without the full metadata.

use crate::state::{AppState, RelayChainError, SubstrateLegacyRpc};
use crate::utils::BlockId;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use frame_metadata::RuntimeMetadataPrefixed;
use merkleized_metadata::{
    ExtraInfo, SignedExtrinsicData, generate_metadata_digest, generate_proof_for_extrinsic,
    generate_proof_for_extrinsic_parts,
};
use parity_scale_codec::{Decode, Encode};
use serde::{Deserialize, Serialize};
use subxt::SubstrateConfig;
use subxt::client::OnlineClientAtBlock;
use thiserror::Error;

const DEFAULT_DECIMALS: u8 = 10;
const DEFAULT_SS58_PREFIX: u16 = 42;
const DEFAULT_TOKEN_SYMBOL: &str = "DOT";
const REQUIRED_METADATA_VERSION: u32 = 15;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataBlobRequest {
    /// Full encoded extrinsic (hex). Alternative to callData + includedInExtrinsic + includedInSignedData.
    pub tx: Option<String>,
    /// Additional signed data for the extrinsic (hex). Used with `tx`.
    pub tx_additional_signed: Option<String>,
    /// Call data portion of the extrinsic (hex). Must be used with includedInExtrinsic and includedInSignedData.
    pub call_data: Option<String>,
    /// Signed extension data included in the extrinsic (hex).
    pub included_in_extrinsic: Option<String>,
    /// Signed extension data included in the signature payload (hex).
    pub included_in_signed_data: Option<String>,
    /// Block hash or number to query metadata at. If not provided, uses finalized head.
    pub at: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct At {
    pub hash: String,
    pub height: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetadataBlobResponse {
    pub at: At,
    /// 32-byte metadata hash (hex) for CheckMetadataHash signed extension.
    pub metadata_hash: String,
    /// Minimal metadata proof (hex, SCALE-encoded).
    pub metadata_blob: String,
    /// Chain spec version.
    pub spec_version: u32,
    /// Chain spec name.
    pub spec_name: String,
    /// SS58 address prefix.
    pub base58_prefix: u16,
    /// Native token decimals.
    pub decimals: u8,
    /// Native token symbol.
    pub token_symbol: String,
}

#[derive(Debug, Serialize)]
pub struct MetadataBlobFailure {
    pub code: u16,
    pub error: String,
    pub cause: String,
    pub stack: String,
}

#[derive(Debug, Error)]
pub enum MetadataBlobError {
    #[error(
        "Must provide either `tx` (full extrinsic) or `callData` with `includedInExtrinsic` and `includedInSignedData`."
    )]
    MissingRequiredFields,

    #[error(
        "When using `callData`, must also provide `includedInExtrinsic` and `includedInSignedData`."
    )]
    IncompleteCallDataFields,

    #[error(
        "Metadata V15 is not available on this chain. CheckMetadataHash requires V15 metadata."
    )]
    MetadataV15NotAvailable,

    #[error("Invalid hex encoding in request")]
    InvalidHex { field: String, cause: String },

    #[error("Failed to generate metadata proof")]
    ProofGenerationFailed { cause: String },

    #[error("Invalid block parameter")]
    InvalidBlockParam { cause: String },

    #[error("Block not found")]
    BlockNotFound { cause: String },

    #[error("Failed to fetch chain information")]
    FetchFailed { cause: String, stack: String },

    #[error(transparent)]
    RelayChain(#[from] RelayChainError),
}

impl IntoResponse for MetadataBlobError {
    fn into_response(self) -> axum::response::Response {
        let (status, code, error_msg, cause, stack) = match self {
            MetadataBlobError::MissingRequiredFields => {
                let cause = "Must provide either `tx` (full extrinsic) or `callData` with `includedInExtrinsic` and `includedInSignedData`.".to_string();
                (
                    StatusCode::BAD_REQUEST,
                    400,
                    "Invalid request body",
                    cause.clone(),
                    format!("Error: {}\n    at metadata_blob", cause),
                )
            }
            MetadataBlobError::IncompleteCallDataFields => {
                let cause = "When using `callData`, must also provide `includedInExtrinsic` and `includedInSignedData`.".to_string();
                (
                    StatusCode::BAD_REQUEST,
                    400,
                    "Invalid request body",
                    cause.clone(),
                    format!("Error: {}\n    at metadata_blob", cause),
                )
            }
            MetadataBlobError::MetadataV15NotAvailable => {
                let cause = "Metadata V15 is not available on this chain. CheckMetadataHash requires V15 metadata.".to_string();
                (
                    StatusCode::BAD_REQUEST,
                    400,
                    "Metadata V15 not available",
                    cause.clone(),
                    format!("Error: {}\n    at metadata_blob", cause),
                )
            }
            MetadataBlobError::InvalidHex { field, cause } => {
                let msg = format!("Invalid hex encoding in `{}`: {}", field, cause);
                (
                    StatusCode::BAD_REQUEST,
                    400,
                    "Invalid hex encoding",
                    msg.clone(),
                    format!("Error: {}\n    at metadata_blob", msg),
                )
            }
            MetadataBlobError::ProofGenerationFailed { cause } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                500,
                "Failed to generate metadata proof",
                cause.clone(),
                format!("Error: {}\n    at metadata_blob", cause),
            ),
            MetadataBlobError::InvalidBlockParam { cause } => (
                StatusCode::BAD_REQUEST,
                400,
                "Invalid block parameter",
                cause.clone(),
                format!("Error: {}\n    at metadata_blob", cause),
            ),
            MetadataBlobError::BlockNotFound { cause } => (
                StatusCode::NOT_FOUND,
                404,
                "Block not found",
                cause.clone(),
                format!("Error: {}\n    at metadata_blob", cause),
            ),
            MetadataBlobError::FetchFailed { cause, stack } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                500,
                "Failed to fetch metadata",
                cause,
                stack,
            ),
            MetadataBlobError::RelayChain(ref err) => {
                let status = match err {
                    RelayChainError::NotConfigured => StatusCode::BAD_REQUEST,
                    RelayChainError::ConnectionFailed(_) => StatusCode::SERVICE_UNAVAILABLE,
                };
                let cause = err.to_string();
                (
                    status,
                    status.as_u16(),
                    "Relay chain not available",
                    cause.clone(),
                    format!("Error: {}\n    at metadata_blob_rc", cause),
                )
            }
        };

        let body = Json(MetadataBlobFailure {
            code,
            error: error_msg.to_string(),
            cause,
            stack,
        });
        (status, body).into_response()
    }
}

#[utoipa::path(
    post,
    path = "/v1/transaction/metadata-blob",
    tag = "transaction",
    summary = "Generate metadata blob",
    description = "Generates a metadata blob for transaction signing with the CheckMetadataHash extension.",
    request_body(content = Object, description = "Request with 'tx' field and optional 'at' block"),
    responses(
        (status = 200, description = "Metadata blob", body = Object),
        (status = 400, description = "Invalid parameters"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn metadata_blob(
    State(state): State<AppState>,
    Json(body): Json<MetadataBlobRequest>,
) -> Result<Json<MetadataBlobResponse>, MetadataBlobError> {
    metadata_blob_internal(&state.client, &state.legacy_rpc, body).await
}

#[utoipa::path(
    post,
    path = "/v1/rc/transaction/metadata-blob",
    tag = "rc",
    summary = "RC metadata blob",
    description = "Generates a metadata blob from the relay chain for transaction signing.",
    request_body(content = Object, description = "Request with 'tx' field and optional 'at' block"),
    responses(
        (status = 200, description = "Metadata blob", body = Object),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn metadata_blob_rc(
    State(state): State<AppState>,
    Json(body): Json<MetadataBlobRequest>,
) -> Result<Json<MetadataBlobResponse>, MetadataBlobError> {
    let relay_client = state
        .get_relay_chain_client()
        .ok_or(MetadataBlobError::RelayChain(RelayChainError::NotConfigured))?;
    let relay_legacy_rpc = state
        .get_relay_chain_rpc()
        .await
        .map_err(MetadataBlobError::RelayChain)?;

    metadata_blob_internal(relay_client, &relay_legacy_rpc, body).await
}

async fn metadata_blob_internal(
    client: &subxt::OnlineClient<SubstrateConfig>,
    legacy_rpc: &SubstrateLegacyRpc,
    body: MetadataBlobRequest,
) -> Result<Json<MetadataBlobResponse>, MetadataBlobError> {
    // Validate request: need either tx OR (callData + includedInExtrinsic + includedInSignedData)
    let extrinsic_input = validate_request(&body)?;

    // Resolve block
    let client_at =
        match &body.at {
            None => client.at_current_block().await.map_err(|e| {
                let cause = e.to_string();
                MetadataBlobError::FetchFailed {
                    cause: cause.clone(),
                    stack: format!("Error: {}\n    at metadata_blob", cause),
                }
            })?,
            Some(at_str) => {
                let block_id = at_str.parse::<BlockId>().map_err(|e| {
                    MetadataBlobError::InvalidBlockParam {
                        cause: e.to_string(),
                    }
                })?;
                match block_id {
                    BlockId::Hash(hash) => client.at_block(hash).await.map_err(|e| {
                        MetadataBlobError::BlockNotFound {
                            cause: e.to_string(),
                        }
                    })?,
                    BlockId::Number(num) => client.at_block(num).await.map_err(|e| {
                        MetadataBlobError::BlockNotFound {
                            cause: e.to_string(),
                        }
                    })?,
                }
            }
        };

    let block_hash = client_at.block_hash();
    let block_hash_str = format!("{:#x}", block_hash);
    let block_number = client_at.block_number().to_string();
    let spec_version = client_at.spec_version();

    // Get available metadata versions using call_raw
    let available_versions = fetch_metadata_versions(&client_at).await?;

    // Check if V15 is available
    if !available_versions.contains(&REQUIRED_METADATA_VERSION) {
        return Err(MetadataBlobError::MetadataV15NotAvailable);
    }

    // Fetch metadata V15 using call_raw
    let metadata_bytes = fetch_metadata_at_version(&client_at, REQUIRED_METADATA_VERSION).await?;

    // Decode metadata
    let metadata_prefixed =
        RuntimeMetadataPrefixed::decode(&mut &metadata_bytes[..]).map_err(|e| {
            let cause = format!("Failed to decode metadata: {}", e);
            MetadataBlobError::FetchFailed {
                cause: cause.clone(),
                stack: format!("Error: {}\n    at metadata_blob (decode)", cause),
            }
        })?;

    // Get runtime version for spec_name using legacy RPC (typed)
    let runtime_version = legacy_rpc
        .state_get_runtime_version(Some(block_hash))
        .await
        .map_err(|e| {
            let cause = e.to_string();
            MetadataBlobError::FetchFailed {
                cause: cause.clone(),
                stack: format!("Error: {}\n    at metadata_blob (runtime version)", cause),
            }
        })?;

    let spec_name = runtime_version
        .other
        .get("specName")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    // Get SS58 prefix from constants
    let base58_prefix = fetch_ss58_prefix(&client_at);

    // Get chain properties using legacy RPC (typed)
    let properties = legacy_rpc.system_properties().await.map_err(|e| {
        let cause = e.to_string();
        MetadataBlobError::FetchFailed {
            cause: cause.clone(),
            stack: format!("Error: {}\n    at metadata_blob (properties)", cause),
        }
    })?;

    let decimals = extract_decimals(&properties);
    let token_symbol = extract_token_symbol(&properties);

    // Build ExtraInfo for merkleization
    let extra_info = ExtraInfo {
        spec_version,
        spec_name: spec_name.clone(),
        base58_prefix,
        decimals,
        token_symbol: token_symbol.clone(),
    };

    // Generate metadata digest
    let metadata_digest =
        generate_metadata_digest(&metadata_prefixed.1, extra_info).map_err(|e| {
            MetadataBlobError::ProofGenerationFailed {
                cause: format!("Failed to generate metadata digest: {}", e),
            }
        })?;

    let metadata_hash = format!("0x{}", hex::encode(metadata_digest.hash()));

    // Generate proof based on input type
    let proof = match extrinsic_input {
        ExtrinsicInput::Full {
            tx,
            tx_additional_signed,
        } => {
            generate_proof_for_extrinsic(&tx, tx_additional_signed.as_deref(), &metadata_prefixed.1)
                .map_err(|e| MetadataBlobError::ProofGenerationFailed {
                    cause: format!("Failed to generate proof for extrinsic: {}", e),
                })?
        }
        ExtrinsicInput::Parts {
            call_data,
            included_in_extrinsic,
            included_in_signed_data,
        } => {
            let signed_ext_data = SignedExtrinsicData {
                included_in_extrinsic: &included_in_extrinsic,
                included_in_signed_data: &included_in_signed_data,
            };
            generate_proof_for_extrinsic_parts(
                &call_data,
                Some(signed_ext_data),
                &metadata_prefixed.1,
            )
            .map_err(|e| MetadataBlobError::ProofGenerationFailed {
                cause: format!("Failed to generate proof for extrinsic parts: {}", e),
            })?
        }
    };

    let metadata_blob = format!("0x{}", hex::encode(proof.encode()));

    Ok(Json(MetadataBlobResponse {
        at: At {
            hash: block_hash_str,
            height: block_number,
        },
        metadata_hash,
        metadata_blob,
        spec_version,
        spec_name,
        base58_prefix,
        decimals,
        token_symbol,
    }))
}

/// Fetch available metadata versions using typed runtime API
async fn fetch_metadata_versions(
    client_at: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<u32>, MetadataBlobError> {
    let method =
        subxt::dynamic::runtime_api_call::<_, Vec<u32>>("Metadata", "metadata_versions", ());

    client_at.runtime_apis().call(method).await.map_err(|e| {
        let cause = e.to_string();
        MetadataBlobError::FetchFailed {
            cause: cause.clone(),
            stack: format!("Error: {}\n    at metadata_blob (metadata_versions)", cause),
        }
    })
}

/// Fetch metadata at a specific version using typed runtime API
async fn fetch_metadata_at_version(
    client_at: &OnlineClientAtBlock<SubstrateConfig>,
    version: u32,
) -> Result<Vec<u8>, MetadataBlobError> {
    let method = subxt::dynamic::runtime_api_call::<_, Option<Vec<u8>>>(
        "Metadata",
        "metadata_at_version",
        (version,),
    );

    let metadata_opt = client_at.runtime_apis().call(method).await.map_err(|e| {
        let cause = e.to_string();
        MetadataBlobError::FetchFailed {
            cause: cause.clone(),
            stack: format!(
                "Error: {}\n    at metadata_blob (metadata_at_version)",
                cause
            ),
        }
    })?;

    metadata_opt.ok_or(MetadataBlobError::MetadataV15NotAvailable)
}

/// Fetch SS58 prefix from System::SS58Prefix constant
fn fetch_ss58_prefix(client_at: &OnlineClientAtBlock<SubstrateConfig>) -> u16 {
    let ss58_addr = subxt::dynamic::constant::<u16>("System", "SS58Prefix");
    client_at
        .constants()
        .entry(ss58_addr)
        .unwrap_or(DEFAULT_SS58_PREFIX)
}

enum ExtrinsicInput {
    Full {
        tx: Vec<u8>,
        tx_additional_signed: Option<Vec<u8>>,
    },
    Parts {
        call_data: Vec<u8>,
        included_in_extrinsic: Vec<u8>,
        included_in_signed_data: Vec<u8>,
    },
}

fn validate_request(body: &MetadataBlobRequest) -> Result<ExtrinsicInput, MetadataBlobError> {
    // Check if tx is provided
    if let Some(tx) = &body.tx
        && !tx.is_empty()
    {
        let tx_bytes = decode_hex(tx, "tx")?;
        let tx_additional_signed = body
            .tx_additional_signed
            .as_ref()
            .map(|s| decode_hex(s, "txAdditionalSigned"))
            .transpose()?;
        return Ok(ExtrinsicInput::Full {
            tx: tx_bytes,
            tx_additional_signed,
        });
    }

    // Check if callData is provided
    if let Some(call_data) = &body.call_data
        && !call_data.is_empty()
    {
        // Must also have includedInExtrinsic and includedInSignedData
        let included_in_extrinsic = body
            .included_in_extrinsic
            .as_ref()
            .filter(|s| !s.is_empty())
            .ok_or(MetadataBlobError::IncompleteCallDataFields)?;
        let included_in_signed_data = body
            .included_in_signed_data
            .as_ref()
            .filter(|s| !s.is_empty())
            .ok_or(MetadataBlobError::IncompleteCallDataFields)?;

        let call_data_bytes = decode_hex(call_data, "callData")?;
        let included_in_extrinsic_bytes = decode_hex(included_in_extrinsic, "includedInExtrinsic")?;
        let included_in_signed_data_bytes =
            decode_hex(included_in_signed_data, "includedInSignedData")?;

        return Ok(ExtrinsicInput::Parts {
            call_data: call_data_bytes,
            included_in_extrinsic: included_in_extrinsic_bytes,
            included_in_signed_data: included_in_signed_data_bytes,
        });
    }

    Err(MetadataBlobError::MissingRequiredFields)
}

fn decode_hex(s: &str, field: &str) -> Result<Vec<u8>, MetadataBlobError> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    hex::decode(s).map_err(|e| MetadataBlobError::InvalidHex {
        field: field.to_string(),
        cause: e.to_string(),
    })
}

/// System properties from RPC (serde_json::Map from legacy_rpc.system_properties())
type SystemProperties = serde_json::Map<String, serde_json::Value>;

fn extract_decimals(properties: &SystemProperties) -> u8 {
    properties
        .get("tokenDecimals")
        .and_then(|v| {
            // Could be a single number or an array
            if let Some(n) = v.as_u64() {
                Some(n as u8)
            } else if let Some(arr) = v.as_array() {
                arr.first().and_then(|n| n.as_u64()).map(|n| n as u8)
            } else {
                None
            }
        })
        .unwrap_or(DEFAULT_DECIMALS)
}

fn extract_token_symbol(properties: &SystemProperties) -> String {
    properties
        .get("tokenSymbol")
        .and_then(|v| {
            // Could be a single string or an array
            if let Some(s) = v.as_str() {
                Some(s.to_string())
            } else if let Some(arr) = v.as_array() {
                arr.first().and_then(|s| s.as_str()).map(|s| s.to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| DEFAULT_TOKEN_SYMBOL.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_metadata_blob_response_serialization() {
        let response = MetadataBlobResponse {
            at: At {
                hash: "0x1234".to_string(),
                height: "100".to_string(),
            },
            metadata_hash: "0xabcd".to_string(),
            metadata_blob: "0xdeadbeef".to_string(),
            spec_version: 1000000,
            spec_name: "polkadot".to_string(),
            base58_prefix: 0,
            decimals: 10,
            token_symbol: "DOT".to_string(),
        };
        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["at"]["hash"], "0x1234");
        assert_eq!(json["metadataHash"], "0xabcd");
        assert_eq!(json["metadataBlob"], "0xdeadbeef");
        assert_eq!(json["specVersion"], 1000000);
        assert_eq!(json["specName"], "polkadot");
        assert_eq!(json["base58Prefix"], 0);
        assert_eq!(json["decimals"], 10);
        assert_eq!(json["tokenSymbol"], "DOT");
    }

    #[test]
    fn test_validate_request_with_tx() {
        let body = MetadataBlobRequest {
            tx: Some("0x1234".to_string()),
            tx_additional_signed: None,
            call_data: None,
            included_in_extrinsic: None,
            included_in_signed_data: None,
            at: None,
        };
        let result = validate_request(&body);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_request_with_parts() {
        let body = MetadataBlobRequest {
            tx: None,
            tx_additional_signed: None,
            call_data: Some("0x0403".to_string()),
            included_in_extrinsic: Some("0x00".to_string()),
            included_in_signed_data: Some("0x00".to_string()),
            at: None,
        };
        let result = validate_request(&body);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_request_missing_fields() {
        let body = MetadataBlobRequest {
            tx: None,
            tx_additional_signed: None,
            call_data: None,
            included_in_extrinsic: None,
            included_in_signed_data: None,
            at: None,
        };
        let result = validate_request(&body);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_request_incomplete_parts() {
        let body = MetadataBlobRequest {
            tx: None,
            tx_additional_signed: None,
            call_data: Some("0x0403".to_string()),
            included_in_extrinsic: None,
            included_in_signed_data: None,
            at: None,
        };
        let result = validate_request(&body);
        assert!(matches!(
            result,
            Err(MetadataBlobError::IncompleteCallDataFields)
        ));
    }

    #[test]
    fn test_extract_decimals_single() {
        let mut props = serde_json::Map::new();
        props.insert("tokenDecimals".to_string(), json!(12));
        assert_eq!(extract_decimals(&props), 12);
    }

    #[test]
    fn test_extract_decimals_array() {
        let mut props = serde_json::Map::new();
        props.insert("tokenDecimals".to_string(), json!([10, 6]));
        assert_eq!(extract_decimals(&props), 10);
    }

    #[test]
    fn test_extract_decimals_default() {
        let props = serde_json::Map::new();
        assert_eq!(extract_decimals(&props), DEFAULT_DECIMALS);
    }

    #[test]
    fn test_extract_token_symbol_single() {
        let mut props = serde_json::Map::new();
        props.insert("tokenSymbol".to_string(), json!("KSM"));
        assert_eq!(extract_token_symbol(&props), "KSM");
    }

    #[test]
    fn test_extract_token_symbol_array() {
        let mut props = serde_json::Map::new();
        props.insert("tokenSymbol".to_string(), json!(["DOT", "USDT"]));
        assert_eq!(extract_token_symbol(&props), "DOT");
    }

    #[test]
    fn test_extract_token_symbol_default() {
        let props = serde_json::Map::new();
        assert_eq!(extract_token_symbol(&props), DEFAULT_TOKEN_SYMBOL);
    }

    #[test]
    fn test_decode_hex_with_prefix() {
        let result = decode_hex("0x1234", "test");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec![0x12, 0x34]);
    }

    #[test]
    fn test_decode_hex_without_prefix() {
        let result = decode_hex("abcd", "test");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec![0xab, 0xcd]);
    }

    #[test]
    fn test_decode_hex_invalid() {
        let result = decode_hex("not_hex", "test");
        assert!(result.is_err());
    }
}
