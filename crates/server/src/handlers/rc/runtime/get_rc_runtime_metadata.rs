// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::extractors::JsonQuery;
use crate::handlers::runtime::{RuntimeMetadataResponse, VERSION_REGEX, convert_metadata};
use crate::state::{AppState, RelayChainError, SubstrateLegacyRpc};
use crate::utils;
use axum::{Json, extract::Path, extract::State, http::StatusCode, response::IntoResponse};
use frame_metadata::RuntimeMetadataPrefixed;
use parity_scale_codec::Decode;
use serde_json::json;
use subxt_rpcs::{RpcClient, rpc_params};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GetRcMetadataError {
    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] crate::utils::BlockIdParseError),

    #[error(transparent)]
    RelayChain(#[from] RelayChainError),

    #[error("Failed to get metadata from RPC")]
    RpcFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to decode metadata hex")]
    HexDecodeFailed(#[source] hex::FromHexError),

    #[error("Failed to decode metadata SCALE")]
    ScaleDecodeFailed(#[source] parity_scale_codec::Error),

    #[error("Metadata bytes too short")]
    MetadataTooShort,

    #[error("Unsupported metadata version")]
    UnsupportedVersion,

    #[error(
        "Invalid version format: {0}. Expected format 'vX' where X is a number (e.g., 'v14', 'v15')"
    )]
    InvalidVersionFormat(String),

    #[error("Metadata version {0} is not available")]
    VersionNotAvailable(u32),

    #[error("Function 'metadata.metadataVersions()' is not available at this block height")]
    MetadataVersionsNotAvailable,

    #[error("Service temporarily unavailable: {0}")]
    ServiceUnavailable(String),

    #[error("Block not found: {0}")]
    BlockNotFound(String),
}

impl IntoResponse for GetRcMetadataError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            GetRcMetadataError::InvalidBlockParam(_)
            | GetRcMetadataError::InvalidVersionFormat(_)
            | GetRcMetadataError::VersionNotAvailable(_)
            | GetRcMetadataError::MetadataVersionsNotAvailable
            | GetRcMetadataError::BlockNotFound(_)
            | GetRcMetadataError::RelayChain(RelayChainError::NotConfigured) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            GetRcMetadataError::RelayChain(RelayChainError::ConnectionFailed(_))
            | GetRcMetadataError::ServiceUnavailable(_) => {
                (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
            }
            GetRcMetadataError::RpcFailed(err) => utils::rpc_error_to_status(err),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AtBlockParam {
    pub at: Option<String>,
}

/// Resolve a block identifier to a block hash string using relay chain RPC.
async fn resolve_relay_block_hash(
    relay_rpc_client: &RpcClient,
    relay_legacy_rpc: &SubstrateLegacyRpc,
    at: Option<&str>,
) -> Result<String, GetRcMetadataError> {
    match at {
        None => {
            let hash = relay_legacy_rpc
                .chain_get_finalized_head()
                .await
                .map_err(GetRcMetadataError::RpcFailed)?;
            Ok(format!("{:#x}", hash))
        }
        Some(at_str) => {
            let block_id = at_str.parse::<crate::utils::BlockId>()?;
            match block_id {
                crate::utils::BlockId::Hash(hash) => Ok(format!("{:#x}", hash)),
                crate::utils::BlockId::Number(number) => {
                    let hash: Option<String> = relay_rpc_client
                        .request("chain_getBlockHash", rpc_params![number])
                        .await
                        .map_err(GetRcMetadataError::RpcFailed)?;
                    hash.ok_or_else(|| {
                        GetRcMetadataError::BlockNotFound(format!(
                            "Block at height {} not found",
                            number
                        ))
                    })
                }
            }
        }
    }
}

/// Handler for GET /rc/runtime/metadata
///
/// Returns the decoded runtime metadata of the relay chain in JSON format.
///
/// Query parameters:
/// - `at` (optional): Block identifier (block number or block hash). Defaults to latest block.
#[utoipa::path(
    get,
    path = "/v1/rc/runtime/metadata",
    tag = "rc",
    summary = "RC get runtime metadata",
    description = "Returns the decoded runtime metadata of the relay chain in JSON format.",
    params(
        ("at" = Option<String>, description = "Block identifier (number or hash)")
    ),
    responses(
        (status = 200, description = "Relay chain runtime metadata", body = Object),
        (status = 503, description = "Relay chain not configured"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_rc_runtime_metadata(
    State(state): State<AppState>,
    JsonQuery(params): JsonQuery<AtBlockParam>,
) -> Result<Json<RuntimeMetadataResponse>, GetRcMetadataError> {
    let relay_rpc_client = state.get_relay_chain_rpc_client().await?;
    let relay_legacy_rpc = state.get_relay_chain_rpc().await?;

    let block_hash =
        resolve_relay_block_hash(&relay_rpc_client, &relay_legacy_rpc, params.at.as_deref())
            .await?;

    let metadata_hex: String = relay_rpc_client
        .request("state_getMetadata", rpc_params![&block_hash])
        .await
        .map_err(GetRcMetadataError::RpcFailed)?;

    let hex_str = metadata_hex.strip_prefix("0x").unwrap_or(&metadata_hex);
    let metadata_bytes = hex::decode(hex_str).map_err(GetRcMetadataError::HexDecodeFailed)?;

    if metadata_bytes.len() < 4 {
        return Err(GetRcMetadataError::MetadataTooShort);
    }

    let magic_number = u32::from_le_bytes(
        metadata_bytes[0..4]
            .try_into()
            .map_err(|_| GetRcMetadataError::MetadataTooShort)?,
    );

    let metadata_prefixed = RuntimeMetadataPrefixed::decode(&mut &metadata_bytes[..])
        .map_err(GetRcMetadataError::ScaleDecodeFailed)?;

    let metadata = convert_metadata(&metadata_prefixed.1)
        .map_err(|_| GetRcMetadataError::UnsupportedVersion)?;

    Ok(Json(RuntimeMetadataResponse {
        magic_number: magic_number.to_string(),
        metadata,
    }))
}

/// Handler for GET /rc/runtime/metadata/versions
///
/// Returns the available metadata versions on the relay chain at a given block.
///
/// Query parameters:
/// - `at` (optional): Block identifier (block number or block hash). Defaults to latest block.
#[utoipa::path(
    get,
    path = "/v1/rc/runtime/metadata/versions",
    tag = "rc",
    summary = "RC get metadata versions",
    description = "Returns the available metadata versions on the relay chain at a given block.",
    params(
        ("at" = Option<String>, description = "Block identifier (number or hash)")
    ),
    responses(
        (status = 200, description = "Available metadata versions", body = Object),
        (status = 503, description = "Relay chain not configured"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_rc_runtime_metadata_versions(
    State(state): State<AppState>,
    JsonQuery(params): JsonQuery<AtBlockParam>,
) -> Result<Json<Vec<String>>, GetRcMetadataError> {
    let relay_rpc_client = state.get_relay_chain_rpc_client().await?;
    let relay_legacy_rpc = state.get_relay_chain_rpc().await?;

    let block_hash =
        resolve_relay_block_hash(&relay_rpc_client, &relay_legacy_rpc, params.at.as_deref())
            .await?;

    let call_data = "0x".to_string();

    let result: String = relay_rpc_client
        .request(
            "state_call",
            rpc_params!["Metadata_metadata_versions", &call_data, &block_hash],
        )
        .await
        .map_err(|e| {
            let err_str = e.to_string().to_lowercase();
            if err_str.contains("not found") || err_str.contains("does not exist") {
                GetRcMetadataError::MetadataVersionsNotAvailable
            } else {
                GetRcMetadataError::RpcFailed(e)
            }
        })?;

    let hex_str = result.strip_prefix("0x").unwrap_or(&result);
    let bytes = hex::decode(hex_str).map_err(GetRcMetadataError::HexDecodeFailed)?;

    let versions: Vec<u32> =
        Vec::<u32>::decode(&mut &bytes[..]).map_err(GetRcMetadataError::ScaleDecodeFailed)?;

    let version_strings: Vec<String> = versions.iter().map(|v| format!("{}", v)).collect();

    Ok(Json(version_strings))
}

/// Handler for GET /rc/runtime/metadata/{version}
///
/// Returns the relay chain metadata at a specific version.
/// The version parameter should be in "vX" format (e.g., "v14", "v15").
///
/// Query parameters:
/// - `at` (optional): Block identifier (block number or block hash). Defaults to latest block.
#[utoipa::path(
    get,
    path = "/v1/rc/runtime/metadata/{version}",
    tag = "rc",
    summary = "RC get metadata by version",
    description = "Returns the relay chain metadata at a specific version (e.g., v14, v15).",
    params(
        ("version" = String, Path, description = "Metadata version in 'vX' format (e.g., v14, v15)"),
        ("at" = Option<String>, description = "Block identifier (number or hash)")
    ),
    responses(
        (status = 200, description = "Relay chain metadata at specified version", body = Object),
        (status = 400, description = "Invalid version format"),
        (status = 503, description = "Relay chain not configured"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_rc_runtime_metadata_versioned(
    State(state): State<AppState>,
    Path(version): Path<String>,
    JsonQuery(params): JsonQuery<AtBlockParam>,
) -> Result<Json<RuntimeMetadataResponse>, GetRcMetadataError> {
    let version_num: u32 = match VERSION_REGEX.captures(&version) {
        Some(caps) => caps
            .get(1)
            .expect("regex capture group 1 must exist")
            .as_str()
            .parse()
            .map_err(|_| GetRcMetadataError::InvalidVersionFormat(version.clone()))?,
        None => return Err(GetRcMetadataError::InvalidVersionFormat(version.clone())),
    };

    let relay_rpc_client = state.get_relay_chain_rpc_client().await?;
    let relay_legacy_rpc = state.get_relay_chain_rpc().await?;

    let block_hash =
        resolve_relay_block_hash(&relay_rpc_client, &relay_legacy_rpc, params.at.as_deref())
            .await?;

    // First, check if the version is available
    let versions_call_data = "0x".to_string();
    let versions_result: String = relay_rpc_client
        .request(
            "state_call",
            rpc_params![
                "Metadata_metadata_versions",
                &versions_call_data,
                &block_hash
            ],
        )
        .await
        .map_err(|e| {
            let err_str = e.to_string().to_lowercase();
            if err_str.contains("not found") || err_str.contains("does not exist") {
                GetRcMetadataError::MetadataVersionsNotAvailable
            } else {
                GetRcMetadataError::RpcFailed(e)
            }
        })?;

    let versions_hex = versions_result
        .strip_prefix("0x")
        .unwrap_or(&versions_result);
    let versions_bytes = hex::decode(versions_hex).map_err(GetRcMetadataError::HexDecodeFailed)?;
    let available_versions: Vec<u32> = Vec::<u32>::decode(&mut &versions_bytes[..])
        .map_err(GetRcMetadataError::ScaleDecodeFailed)?;

    if !available_versions.contains(&version_num) {
        return Err(GetRcMetadataError::VersionNotAvailable(version_num));
    }

    // Call state_call with Metadata_metadata_at_version
    let version_encoded = parity_scale_codec::Encode::encode(&version_num);
    let call_data = format!("0x{}", hex::encode(&version_encoded));

    let result: String = relay_rpc_client
        .request(
            "state_call",
            rpc_params!["Metadata_metadata_at_version", &call_data, &block_hash],
        )
        .await
        .map_err(|e| {
            let err_str = e.to_string().to_lowercase();
            if err_str.contains("not found") || err_str.contains("does not exist") {
                GetRcMetadataError::MetadataVersionsNotAvailable
            } else {
                GetRcMetadataError::RpcFailed(e)
            }
        })?;

    // Decode the result - it's an Option<OpaqueMetadata> where OpaqueMetadata is Vec<u8>
    // Option encoding: 0x00 = None, 0x01 + data = Some
    let hex_str = result.strip_prefix("0x").unwrap_or(&result);
    let bytes = hex::decode(hex_str).map_err(GetRcMetadataError::HexDecodeFailed)?;

    if bytes.is_empty() || bytes[0] == 0 {
        return Err(GetRcMetadataError::VersionNotAvailable(version_num));
    }

    // Skip the Option Some byte (0x01) and decode the inner Vec<u8>
    let opaque_metadata: Vec<u8> =
        Vec::<u8>::decode(&mut &bytes[1..]).map_err(GetRcMetadataError::ScaleDecodeFailed)?;

    if opaque_metadata.len() < 4 {
        return Err(GetRcMetadataError::MetadataTooShort);
    }

    let magic_number = u32::from_le_bytes(
        opaque_metadata[0..4]
            .try_into()
            .map_err(|_| GetRcMetadataError::MetadataTooShort)?,
    );

    let metadata_prefixed = RuntimeMetadataPrefixed::decode(&mut &opaque_metadata[..])
        .map_err(GetRcMetadataError::ScaleDecodeFailed)?;

    let metadata = convert_metadata(&metadata_prefixed.1)
        .map_err(|_| GetRcMetadataError::UnsupportedVersion)?;

    Ok(Json(RuntimeMetadataResponse {
        magic_number: magic_number.to_string(),
        metadata,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_at_block_param_rejects_unknown_fields() {
        let json = r#"{"at": "123", "unknownField": true}"#;
        let result: Result<AtBlockParam, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }
}
