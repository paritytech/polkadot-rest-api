// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::state::AppState;
use crate::utils::BlockId;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use parity_scale_codec::Decode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MaterialQuery {
    /// Block hash or number to query at. If not provided, queries finalized head.
    pub at: Option<String>,
    /// DEPRECATED: If true, metadata is not included. Use `metadata` param instead.
    pub no_meta: Option<bool>,
    /// Metadata format: "json" or "scale". If not provided, metadata is not included.
    pub metadata: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct At {
    pub hash: String,
    pub height: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialResponse {
    pub at: At,
    pub genesis_hash: String,
    pub chain_name: String,
    pub spec_name: String,
    pub spec_version: String,
    pub tx_version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct MaterialFailure {
    pub code: u16,
    pub error: String,
    pub cause: String,
    pub stack: String,
}

#[derive(Debug, Error)]
pub enum MaterialError {
    #[error("Invalid metadata query parameter")]
    InvalidMetadataParam { value: String },

    #[error("Invalid metadata version format")]
    InvalidMetadataVersionFormat { value: String },

    #[error("Metadata version not available")]
    MetadataVersionNotAvailable { version: u32 },

    #[error("Metadata versions API not available")]
    MetadataVersionsApiNotAvailable,

    #[error("Invalid block parameter")]
    InvalidBlockParam { cause: String },

    #[error("Block not found")]
    BlockNotFound { cause: String },

    #[error("Failed to fetch chain information")]
    FetchFailed { cause: String, stack: String },

    #[error("Relay chain not configured")]
    RelayChainNotConfigured,
}

impl IntoResponse for MaterialError {
    fn into_response(self) -> axum::response::Response {
        let (status, code, error_msg, cause, stack) = match self {
            MaterialError::InvalidMetadataParam { value } => {
                let cause = format!(
                    "Invalid value '{}' for the `metadata` query param. Options are `scale` or `json`.",
                    value
                );
                (
                    StatusCode::BAD_REQUEST,
                    400,
                    "Invalid query parameter",
                    cause.clone(),
                    format!("Error: {}\n    at material", cause),
                )
            }
            MaterialError::InvalidMetadataVersionFormat { value } => {
                let cause = format!(
                    "{} input is not of the expected 'vX' format, where 'X' represents the version number (examples: 'v14', 'v15').",
                    value
                );
                (
                    StatusCode::BAD_REQUEST,
                    400,
                    "Invalid metadata version format",
                    cause.clone(),
                    format!("Error: {}\n    at material", cause),
                )
            }
            MaterialError::MetadataVersionNotAvailable { version } => {
                let cause = format!("Version {} of Metadata is not available.", version);
                (
                    StatusCode::BAD_REQUEST,
                    400,
                    "Metadata version not available",
                    cause.clone(),
                    format!("Error: {}\n    at material", cause),
                )
            }
            MaterialError::MetadataVersionsApiNotAvailable => {
                let cause =
                    "Function 'metadata.metadataVersions()' is not available at this block height."
                        .to_string();
                (
                    StatusCode::BAD_REQUEST,
                    400,
                    "Metadata versions API not available",
                    cause.clone(),
                    format!("Error: {}\n    at material", cause),
                )
            }
            MaterialError::InvalidBlockParam { cause } => (
                StatusCode::BAD_REQUEST,
                400,
                "Invalid block parameter",
                cause.clone(),
                format!("Error: {}\n    at material", cause),
            ),
            MaterialError::BlockNotFound { cause } => (
                StatusCode::NOT_FOUND,
                404,
                "Block not found",
                cause.clone(),
                format!("Error: {}\n    at material", cause),
            ),
            MaterialError::FetchFailed { cause, stack } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                500,
                "Failed to fetch transaction material",
                cause,
                stack,
            ),
            MaterialError::RelayChainNotConfigured => {
                let cause = "Relay chain not configured".to_string();
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    503,
                    "Failed to fetch transaction material",
                    cause.clone(),
                    format!("Error: {}\n    at material", cause),
                )
            }
        };

        let body = Json(MaterialFailure {
            code,
            error: error_msg.to_string(),
            cause,
            stack,
        });
        (status, body).into_response()
    }
}

impl From<subxt::error::RuntimeApiError> for MaterialError {
    fn from(err: subxt::error::RuntimeApiError) -> Self {
        let cause = err.to_string();
        MaterialError::FetchFailed {
            cause: cause.clone(),
            stack: format!("Error: {}\n    at material (runtime api)", cause),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataFormat {
    Json,
    Scale,
}

/// Parse metadata query parameters.
/// - If `no_meta` is true (deprecated), no metadata is returned.
/// - Otherwise, if `metadata` is "json" or "scale", that format is returned.
/// - If neither is specified, no metadata is returned.
fn parse_metadata_params(
    metadata: &Option<String>,
    no_meta: Option<bool>,
) -> Result<Option<MetadataFormat>, MaterialError> {
    // noMeta=true takes precedence (deprecated but supported for backwards compatibility)
    if no_meta == Some(true) {
        return Ok(None);
    }

    match metadata {
        None => Ok(None),
        Some(value) => match value.to_lowercase().as_str() {
            "json" => Ok(Some(MetadataFormat::Json)),
            "scale" => Ok(Some(MetadataFormat::Scale)),
            _ => Err(MaterialError::InvalidMetadataParam {
                value: value.clone(),
            }),
        },
    }
}

#[utoipa::path(
    get,
    path = "/v1/transaction/material",
    tag = "transaction",
    summary = "Transaction construction material",
    description = "Returns network information needed for transaction construction including genesis hash, spec version, and optionally metadata.",
    params(
        ("at" = Option<String>, Query, description = "Block hash or number to query at"),
        ("noMeta" = Option<bool>, Query, description = "DEPRECATED: If true, metadata is not included"),
        ("metadata" = Option<String>, Query, description = "Metadata format: 'json' or 'scale'")
    ),
    responses(
        (status = 200, description = "Transaction material", body = Object),
        (status = 400, description = "Invalid parameters"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn material(
    State(state): State<AppState>,
    Query(query): Query<MaterialQuery>,
) -> Result<Json<MaterialResponse>, MaterialError> {
    material_internal(&state.client, &state.rpc_client, query).await
}

#[utoipa::path(
    get,
    path = "/v1/rc/transaction/material",
    tag = "rc",
    summary = "RC transaction material",
    description = "Returns relay chain network information for transaction construction.",
    params(
        ("at" = Option<String>, Query, description = "Block hash or number to query at"),
        ("noMeta" = Option<bool>, Query, description = "DEPRECATED: If true, metadata is not included"),
        ("metadata" = Option<String>, Query, description = "Metadata format: 'json' or 'scale'")
    ),
    responses(
        (status = 200, description = "Relay chain transaction material", body = Object),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn material_rc(
    State(state): State<AppState>,
    Query(query): Query<MaterialQuery>,
) -> Result<Json<MaterialResponse>, MaterialError> {
    let relay_client = state
        .get_relay_chain_client()
        .ok_or(MaterialError::RelayChainNotConfigured)?;
    let relay_rpc_client = state
        .get_relay_chain_rpc_client()
        .ok_or(MaterialError::RelayChainNotConfigured)?;

    material_internal(relay_client, relay_rpc_client, query).await
}

/// Parse and validate metadata version from path parameter.
/// Expected format: "vX" where X is a number (e.g., "v14", "v15").
fn parse_metadata_version(version_str: &str) -> Result<u32, MaterialError> {
    // Check format with regex-like validation
    let version_str = version_str.trim();
    if !version_str.starts_with('v') && !version_str.starts_with('V') {
        return Err(MaterialError::InvalidMetadataVersionFormat {
            value: version_str.to_string(),
        });
    }

    let num_str = &version_str[1..];
    num_str
        .parse::<u32>()
        .map_err(|_| MaterialError::InvalidMetadataVersionFormat {
            value: version_str.to_string(),
        })
}

#[utoipa::path(
    get,
    path = "/v1/transaction/material/{metadataVersion}",
    tag = "transaction",
    summary = "Transaction material with versioned metadata",
    description = "Returns transaction construction material with metadata at a specific version.",
    params(
        ("metadataVersion" = String, Path, description = "Metadata version (e.g., 'v14', 'v15')"),
        ("at" = Option<String>, Query, description = "Block hash or number to query at"),
        ("noMeta" = Option<bool>, Query, description = "DEPRECATED: If true, metadata is not included"),
        ("metadata" = Option<String>, Query, description = "Metadata format: 'json' or 'scale'")
    ),
    responses(
        (status = 200, description = "Transaction material with versioned metadata", body = Object),
        (status = 400, description = "Invalid parameters"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn material_versioned(
    State(state): State<AppState>,
    Path(metadata_version): Path<String>,
    Query(query): Query<MaterialQuery>,
) -> Result<Json<MaterialResponse>, MaterialError> {
    material_versioned_internal(&state.client, &state.rpc_client, metadata_version, query).await
}

#[utoipa::path(
    get,
    path = "/v1/rc/transaction/material/{metadataVersion}",
    tag = "rc",
    summary = "RC transaction material versioned",
    description = "Returns relay chain transaction material with metadata at a specific version.",
    params(
        ("metadataVersion" = String, Path, description = "Metadata version (e.g., 'v14', 'v15')"),
        ("at" = Option<String>, Query, description = "Block hash or number to query at"),
        ("noMeta" = Option<bool>, Query, description = "DEPRECATED: If true, metadata is not included"),
        ("metadata" = Option<String>, Query, description = "Metadata format: 'json' or 'scale'")
    ),
    responses(
        (status = 200, description = "Relay chain transaction material with versioned metadata", body = Object),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn material_versioned_rc(
    State(state): State<AppState>,
    Path(metadata_version): Path<String>,
    Query(query): Query<MaterialQuery>,
) -> Result<Json<MaterialResponse>, MaterialError> {
    let relay_client = state
        .get_relay_chain_client()
        .ok_or(MaterialError::RelayChainNotConfigured)?;
    let relay_rpc_client = state
        .get_relay_chain_rpc_client()
        .ok_or(MaterialError::RelayChainNotConfigured)?;

    material_versioned_internal(relay_client, relay_rpc_client, metadata_version, query).await
}

async fn material_versioned_internal(
    client: &subxt::OnlineClient<subxt::SubstrateConfig>,
    rpc_client: &subxt_rpcs::RpcClient,
    metadata_version_str: String,
    query: MaterialQuery,
) -> Result<Json<MaterialResponse>, MaterialError> {
    let requested_version = parse_metadata_version(&metadata_version_str)?;
    let metadata_format = parse_metadata_params(&query.metadata, query.no_meta)?;

    // Resolve block
    let client_at = match &query.at {
        None => client.at_current_block().await.map_err(|e| {
            let cause = e.to_string();
            MaterialError::FetchFailed {
                cause: cause.clone(),
                stack: format!("Error: {}\n    at material", cause),
            }
        })?,
        Some(at_str) => {
            let block_id =
                at_str
                    .parse::<BlockId>()
                    .map_err(|e| MaterialError::InvalidBlockParam {
                        cause: e.to_string(),
                    })?;

            match block_id {
                BlockId::Hash(hash) => {
                    client
                        .at_block(hash)
                        .await
                        .map_err(|e| MaterialError::BlockNotFound {
                            cause: e.to_string(),
                        })?
                }
                BlockId::Number(num) => {
                    client
                        .at_block(num)
                        .await
                        .map_err(|e| MaterialError::BlockNotFound {
                            cause: e.to_string(),
                        })?
                }
            }
        }
    };

    let block_hash = format!("{:#x}", client_at.block_hash());
    let block_number = client_at.block_number().to_string();
    let spec_version = client_at.spec_version().to_string();
    let tx_version = client_at.transaction_version().to_string();

    // Get available metadata versions
    let versions_method = subxt::dynamic::runtime_api_call::<_, scale_value::Value<()>>(
        "Metadata",
        "metadata_versions",
        (),
    );
    let available_versions_result = client_at.runtime_apis().call(versions_method).await;

    let available_versions: Vec<u32> = match available_versions_result {
        Ok(versions_value) => {
            // Convert scale_value to JSON and extract version numbers
            let versions_json: Value = serde_json::to_value(&versions_value).unwrap_or_default();
            versions_json
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_u64().map(|n| n as u32))
                        .collect()
                })
                .unwrap_or_default()
        }
        Err(_) => {
            return Err(MaterialError::MetadataVersionsApiNotAvailable);
        }
    };

    // Check if requested version is available
    if !available_versions.contains(&requested_version) {
        return Err(MaterialError::MetadataVersionNotAvailable {
            version: requested_version,
        });
    }

    // Get runtime version for spec_name
    let version_method =
        subxt::dynamic::runtime_api_call::<_, scale_value::Value<()>>("Core", "version", ());
    let runtime_version = client_at.runtime_apis().call(version_method).await?;
    let runtime_version_json: Value = serde_json::to_value(&runtime_version).unwrap_or_default();
    let spec_name = runtime_version_json
        .get("spec_name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    // Get genesis hash and chain name
    let genesis_hash = client.genesis_hash().to_string();
    let chain_name: String = rpc_client
        .request("system_chain", subxt_rpcs::rpc_params![])
        .await
        .map_err(|e| {
            let cause = e.to_string();
            MaterialError::FetchFailed {
                cause: cause.clone(),
                stack: format!("Error: {}\n    at material (chain name)", cause),
            }
        })?;

    // Get versioned metadata
    let metadata = if let Some(format) = metadata_format {
        // Call Metadata.metadata_at_version(version)
        let metadata_method = subxt::dynamic::runtime_api_call::<_, scale_value::Value<()>>(
            "Metadata",
            "metadata_at_version",
            (requested_version,),
        );
        let metadata_result = client_at.runtime_apis().call(metadata_method).await?;

        // The result is Option<OpaqueMetadata>, extract the bytes
        let metadata_json: Value = serde_json::to_value(&metadata_result).unwrap_or_default();

        // Handle Option - could be {"Some": [...]} or null
        let metadata_bytes: Option<Vec<u8>> = if let Some(some_value) = metadata_json.get("Some") {
            // Extract bytes from the array
            some_value.as_array().map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_u64().map(|n| n as u8))
                    .collect()
            })
        } else {
            None
        };

        match (format, metadata_bytes) {
            (_, None) => {
                return Err(MaterialError::MetadataVersionNotAvailable {
                    version: requested_version,
                });
            }
            (MetadataFormat::Scale, Some(bytes)) => {
                Some(Value::String(format!("0x{}", hex::encode(&bytes))))
            }
            (MetadataFormat::Json, Some(bytes)) => {
                let metadata = frame_metadata::RuntimeMetadataPrefixed::decode(&mut &bytes[..])
                    .map_err(|e| {
                        let cause = format!("Failed to decode metadata: {}", e);
                        MaterialError::FetchFailed {
                            cause: cause.clone(),
                            stack: format!("Error: {}\n    at material (metadata parse)", cause),
                        }
                    })?;

                let json = serde_json::to_value(&metadata).map_err(|e| {
                    let cause = format!("Failed to serialize metadata to JSON: {}", e);
                    MaterialError::FetchFailed {
                        cause: cause.clone(),
                        stack: format!("Error: {}\n    at material (metadata serialize)", cause),
                    }
                })?;

                Some(json)
            }
        }
    } else {
        None
    };

    Ok(Json(MaterialResponse {
        at: At {
            hash: block_hash,
            height: block_number,
        },
        genesis_hash,
        chain_name,
        spec_name,
        spec_version,
        tx_version,
        metadata,
    }))
}

async fn material_internal(
    client: &subxt::OnlineClient<subxt::SubstrateConfig>,
    rpc_client: &subxt_rpcs::RpcClient,
    query: MaterialQuery,
) -> Result<Json<MaterialResponse>, MaterialError> {
    let metadata_format = parse_metadata_params(&query.metadata, query.no_meta)?;

    // Resolve block
    let (block_hash, block_number, spec_version, tx_version, runtime_version) =
        match &query.at {
            None => {
                // Get current finalized block
                let client_at = client.at_current_block().await.map_err(|e| {
                    let cause = e.to_string();
                    MaterialError::FetchFailed {
                        cause: cause.clone(),
                        stack: format!("Error: {}\n    at material", cause),
                    }
                })?;
                let hash = format!("{:#x}", client_at.block_hash());
                let number = client_at.block_number().to_string();
                let spec_version = client_at.spec_version().to_string();
                let tx_version = client_at.transaction_version().to_string();
                let method = subxt::dynamic::runtime_api_call::<_, scale_value::Value<()>>(
                    "Core",
                    "version",
                    (),
                );
                let runtime_version = client_at.runtime_apis().call(method).await?;

                (hash, number, spec_version, tx_version, runtime_version)
            }
            Some(at_str) => {
                let block_id =
                    at_str
                        .parse::<BlockId>()
                        .map_err(|e| MaterialError::InvalidBlockParam {
                            cause: e.to_string(),
                        })?;

                match block_id {
                    BlockId::Hash(hash) => {
                        let client_at = client.at_block(hash).await.map_err(|e| {
                            MaterialError::BlockNotFound {
                                cause: e.to_string(),
                            }
                        })?;
                        let hash_str = format!("{:#x}", client_at.block_hash());
                        let number = client_at.block_number().to_string();
                        let spec_version = client_at.spec_version().to_string();
                        let tx_version = client_at.transaction_version().to_string();
                        let method = subxt::dynamic::runtime_api_call::<_, scale_value::Value<()>>(
                            "Core",
                            "version",
                            (),
                        );
                        let runtime_version = client_at.runtime_apis().call(method).await?;

                        (hash_str, number, spec_version, tx_version, runtime_version)
                    }
                    BlockId::Number(num) => {
                        let client_at = client.at_block(num).await.map_err(|e| {
                            MaterialError::BlockNotFound {
                                cause: e.to_string(),
                            }
                        })?;
                        let hash_str = format!("{:#x}", client_at.block_hash());
                        let number = client_at.block_number().to_string();
                        let spec_version = client_at.spec_version().to_string();
                        let tx_version = client_at.transaction_version().to_string();
                        let method = subxt::dynamic::runtime_api_call::<_, scale_value::Value<()>>(
                            "Core",
                            "version",
                            (),
                        );
                        let runtime_version = client_at.runtime_apis().call(method).await?;

                        (hash_str, number, spec_version, tx_version, runtime_version)
                    }
                }
            }
        };

    // Get genesis hash
    let genesis_hash = client.genesis_hash().to_string();

    // Get chain name
    let chain_name = rpc_client
        .request("system_chain", subxt_rpcs::rpc_params![])
        .await
        .map_err(|e| {
            let cause = e.to_string();
            MaterialError::FetchFailed {
                cause: cause.clone(),
                stack: format!("Error: {}\n    at material (chain name)", cause),
            }
        })?;

    // Convert scale_value to JSON to extract specName
    let runtime_version_json: Value = serde_json::to_value(&runtime_version).unwrap_or_default();
    let spec_name = runtime_version_json
        .get("spec_name")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    // Get metadata if requested
    let metadata = if let Some(format) = metadata_format {
        let metadata_hex: String = rpc_client
            .request("state_getMetadata", subxt_rpcs::rpc_params![&block_hash])
            .await
            .map_err(|e| {
                let cause = e.to_string();
                MaterialError::FetchFailed {
                    cause: cause.clone(),
                    stack: format!("Error: {}\n    at material (metadata)", cause),
                }
            })?;

        match format {
            MetadataFormat::Scale => Some(Value::String(metadata_hex)),
            MetadataFormat::Json => {
                // Decode the metadata and convert to JSON
                let metadata_bytes =
                    hex::decode(metadata_hex.trim_start_matches("0x")).map_err(|e| {
                        let cause = format!("Failed to decode metadata hex: {}", e);
                        MaterialError::FetchFailed {
                            cause: cause.clone(),
                            stack: format!("Error: {}\n    at material (metadata decode)", cause),
                        }
                    })?;

                let metadata =
                    frame_metadata::RuntimeMetadataPrefixed::decode(&mut &metadata_bytes[..])
                        .map_err(|e| {
                            let cause = format!("Failed to decode metadata: {}", e);
                            MaterialError::FetchFailed {
                                cause: cause.clone(),
                                stack: format!(
                                    "Error: {}\n    at material (metadata parse)",
                                    cause
                                ),
                            }
                        })?;

                let json = serde_json::to_value(&metadata).map_err(|e| {
                    let cause = format!("Failed to serialize metadata to JSON: {}", e);
                    MaterialError::FetchFailed {
                        cause: cause.clone(),
                        stack: format!("Error: {}\n    at material (metadata serialize)", cause),
                    }
                })?;

                Some(json)
            }
        }
    } else {
        None
    };

    Ok(Json(MaterialResponse {
        at: At {
            hash: block_hash,
            height: block_number,
        },
        genesis_hash,
        chain_name,
        spec_name,
        spec_version,
        tx_version,
        metadata,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_metadata_params_none() {
        let result = parse_metadata_params(&None, None);
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_parse_metadata_params_json() {
        let result = parse_metadata_params(&Some("json".to_string()), None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(MetadataFormat::Json));
    }

    #[test]
    fn test_parse_metadata_params_scale() {
        let result = parse_metadata_params(&Some("scale".to_string()), None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(MetadataFormat::Scale));
    }

    #[test]
    fn test_parse_metadata_params_case_insensitive() {
        let result = parse_metadata_params(&Some("JSON".to_string()), None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(MetadataFormat::Json));

        let result = parse_metadata_params(&Some("SCALE".to_string()), None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(MetadataFormat::Scale));
    }

    #[test]
    fn test_parse_metadata_params_invalid() {
        let result = parse_metadata_params(&Some("invalid".to_string()), None);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_metadata_params_no_meta_true() {
        // noMeta=true should return None even if metadata param is set
        let result = parse_metadata_params(&Some("json".to_string()), Some(true));
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_parse_metadata_params_no_meta_false() {
        // noMeta=false should not affect metadata param
        let result = parse_metadata_params(&Some("json".to_string()), Some(false));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Some(MetadataFormat::Json));
    }

    #[test]
    fn test_material_response_serialization() {
        let response = MaterialResponse {
            at: At {
                hash: "0x1234567890abcdef".to_string(),
                height: "12345".to_string(),
            },
            genesis_hash: "0xgenesis".to_string(),
            chain_name: "Polkadot".to_string(),
            spec_name: "polkadot".to_string(),
            spec_version: "1000000".to_string(),
            tx_version: "25".to_string(),
            metadata: None,
        };
        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["at"]["hash"], "0x1234567890abcdef");
        assert_eq!(json["at"]["height"], "12345");
        assert_eq!(json["genesisHash"], "0xgenesis");
        assert_eq!(json["chainName"], "Polkadot");
        assert_eq!(json["specName"], "polkadot");
        assert_eq!(json["specVersion"], "1000000");
        assert_eq!(json["txVersion"], "25");
        assert!(json.get("metadata").is_none());
    }

    #[test]
    fn test_material_response_with_metadata_serialization() {
        let response = MaterialResponse {
            at: At {
                hash: "0x1234".to_string(),
                height: "100".to_string(),
            },
            genesis_hash: "0xgenesis".to_string(),
            chain_name: "Test".to_string(),
            spec_name: "test".to_string(),
            spec_version: "1".to_string(),
            tx_version: "1".to_string(),
            metadata: Some(Value::String("0xmetadata".to_string())),
        };
        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["metadata"], "0xmetadata");
    }

    #[test]
    fn test_material_failure_serialization() {
        let error = MaterialFailure {
            code: 400,
            error: "Invalid query parameter".to_string(),
            cause: "Invalid metadata param".to_string(),
            stack: "Error: Invalid metadata param\n    at material".to_string(),
        };
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["code"], 400);
        assert_eq!(json["error"], "Invalid query parameter");
    }

    #[test]
    fn test_parse_metadata_version_valid() {
        assert_eq!(parse_metadata_version("v14").unwrap(), 14);
        assert_eq!(parse_metadata_version("v15").unwrap(), 15);
        assert_eq!(parse_metadata_version("V14").unwrap(), 14);
        assert_eq!(parse_metadata_version("v0").unwrap(), 0);
    }

    #[test]
    fn test_parse_metadata_version_invalid_format() {
        assert!(parse_metadata_version("14").is_err());
        assert!(parse_metadata_version("version14").is_err());
        assert!(parse_metadata_version("").is_err());
        assert!(parse_metadata_version("vABC").is_err());
        assert!(parse_metadata_version("v-1").is_err());
    }

    #[test]
    fn test_material_query_rejects_unknown_fields() {
        let json = r#"{"at": "123", "unknownField": true}"#;
        let result: Result<MaterialQuery, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }
}
