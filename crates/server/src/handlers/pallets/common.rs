//! Common types and utilities shared across pallet metadata endpoints.
//!
//! This module provides shared error types, query parameters, and response types
//! used by the pallet metadata endpoints (consts, errors, events, dispatchables).

use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur when handling pallet metadata requests.
#[derive(Debug, Error)]
pub enum PalletError {
    /// The block parameter could not be parsed.
    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] crate::utils::BlockIdParseError),

    /// The block could not be resolved (e.g., not found).
    #[error("Block resolution failed")]
    BlockResolveFailed(#[from] crate::utils::BlockResolveError),

    /// Failed to get the client at the specified block.
    #[error("Failed to get client at block")]
    ClientAtBlockFailed(#[from] subxt_historic::error::OnlineClientAtBlockError),

    /// The requested pallet was not found in the metadata.
    #[error("Pallet not found: {0}")]
    PalletNotFound(String),

    /// The metadata version is not supported.
    #[error("Unsupported metadata version")]
    UnsupportedMetadataVersion,

    /// The service is temporarily unavailable (e.g., RPC disconnected).
    #[error("Service temporarily unavailable: {0}")]
    ServiceUnavailable(String),

    /// The `useRcBlock` parameter is only supported for Asset Hub chains.
    #[error("useRcBlock is only supported for Asset Hub chains")]
    UseRcBlockNotSupported,

    /// The relay chain connection is not configured.
    #[error("Relay chain connection not configured")]
    RelayChainNotConfigured,

    /// An error occurred while resolving the relay chain block.
    #[error("RC block error: {0}")]
    RcBlockError(#[from] crate::utils::rc_block::RcBlockError),

    /// The `at` parameter is required when `useRcBlock=true`.
    #[error("at parameter is required when useRcBlock=true")]
    AtParameterRequired,
}

impl IntoResponse for PalletError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            Self::InvalidBlockParam(_) | Self::BlockResolveFailed(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            Self::PalletNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            Self::ClientAtBlockFailed(err) => {
                if crate::utils::is_online_client_at_block_disconnected(err) {
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        format!("Service temporarily unavailable: {err}"),
                    )
                } else {
                    (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
                }
            }
            Self::UnsupportedMetadataVersion | Self::RcBlockError(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            Self::ServiceUnavailable(_) => (StatusCode::SERVICE_UNAVAILABLE, self.to_string()),
            Self::UseRcBlockNotSupported
            | Self::RelayChainNotConfigured
            | Self::AtParameterRequired => (StatusCode::BAD_REQUEST, self.to_string()),
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

// ============================================================================
// Query Parameters
// ============================================================================

/// Query parameters for pallet metadata endpoints.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PalletQueryParams {
    /// Block hash or number to query at. If not provided, uses the latest block.
    pub at: Option<String>,

    /// If `true`, only return the names of items without full metadata.
    #[serde(default)]
    pub only_ids: bool,

    /// If `true`, resolve the block from the relay chain (Asset Hub only).
    #[serde(default)]
    pub use_rc_block: bool,
}

// ============================================================================
// Response Types
// ============================================================================

/// Block reference information in responses.
#[derive(Debug, Clone, Serialize)]
pub struct AtResponse {
    /// The block hash.
    pub hash: String,

    /// The block height (number).
    pub height: String,
}

/// Deprecation information for pallet items.
///
/// This matches the Sidecar response format for deprecation status.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DeprecationInfo {
    /// The item is not deprecated.
    NotDeprecated(Option<()>),

    /// The item is deprecated with optional metadata.
    Deprecated {
        /// A note explaining why the item is deprecated.
        #[serde(skip_serializing_if = "Option::is_none")]
        note: Option<String>,

        /// The version since which the item is deprecated.
        #[serde(skip_serializing_if = "Option::is_none")]
        since: Option<String>,
    },
}

impl Default for DeprecationInfo {
    fn default() -> Self {
        Self::NotDeprecated(None)
    }
}

/// Additional fields for relay chain block responses (Asset Hub only).
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RcBlockFields {
    /// The relay chain block hash.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,

    /// The relay chain block number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,

    /// The Asset Hub timestamp from the resolved block.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}

// ============================================================================
// Pallet Lookup Helpers
// ============================================================================

/// Find a pallet in V14 metadata by name (case-insensitive) or index.
///
/// Returns the pallet name and index if found.
pub fn find_pallet_v14(
    pallets: &[frame_metadata::v14::PalletMetadata<scale_info::form::PortableForm>],
    pallet_id: &str,
) -> Option<(String, u8)> {
    // First, try to parse as a numeric index
    if let Ok(index) = pallet_id.parse::<u8>() {
        for pallet in pallets {
            if pallet.index == index {
                return Some((pallet.name.clone(), pallet.index));
            }
        }
    }

    // Otherwise, search by name (case-insensitive)
    let pallet_id_lower = pallet_id.to_lowercase();
    for pallet in pallets {
        if pallet.name.to_lowercase() == pallet_id_lower {
            return Some((pallet.name.clone(), pallet.index));
        }
    }

    None
}

/// Find a pallet in V15 metadata by name (case-insensitive) or index.
///
/// Returns the pallet name and index if found.
pub fn find_pallet_v15(
    pallets: &[frame_metadata::v15::PalletMetadata<scale_info::form::PortableForm>],
    pallet_id: &str,
) -> Option<(String, u8)> {
    // First, try to parse as a numeric index
    if let Ok(index) = pallet_id.parse::<u8>() {
        for pallet in pallets {
            if pallet.index == index {
                return Some((pallet.name.clone(), pallet.index));
            }
        }
    }

    // Otherwise, search by name (case-insensitive)
    let pallet_id_lower = pallet_id.to_lowercase();
    for pallet in pallets {
        if pallet.name.to_lowercase() == pallet_id_lower {
            return Some((pallet.name.clone(), pallet.index));
        }
    }

    None
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deprecation_info_default() {
        let info = DeprecationInfo::default();
        match info {
            DeprecationInfo::NotDeprecated(None) => {}
            _ => panic!("Expected NotDeprecated(None)"),
        }
    }

    #[test]
    fn test_rc_block_fields_default() {
        let fields = RcBlockFields::default();
        assert!(fields.rc_block_hash.is_none());
        assert!(fields.rc_block_number.is_none());
        assert!(fields.ah_timestamp.is_none());
    }

    #[test]
    fn test_pallet_query_params_defaults() {
        let json = r#"{}"#;
        let params: PalletQueryParams = serde_json::from_str(json).unwrap();
        assert!(params.at.is_none());
        assert!(!params.only_ids);
        assert!(!params.use_rc_block);
    }

    #[test]
    fn test_pallet_query_params_with_values() {
        let json = r#"{"at": "12345", "onlyIds": true, "useRcBlock": true}"#;
        let params: PalletQueryParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.at, Some("12345".to_string()));
        assert!(params.only_ids);
        assert!(params.use_rc_block);
    }

    #[test]
    fn test_at_response_serialization() {
        let at = AtResponse {
            hash: "0x123".to_string(),
            height: "100".to_string(),
        };
        let json = serde_json::to_string(&at).unwrap();
        assert!(json.contains("\"hash\":\"0x123\""));
        assert!(json.contains("\"height\":\"100\""));
    }
}

/// Find a pallet in V16 metadata by name (case-insensitive) or index.
///
/// Returns the pallet name and index if found.
pub fn find_pallet_v16(
    pallets: &[frame_metadata::v16::PalletMetadata<scale_info::form::PortableForm>],
    pallet_id: &str,
) -> Option<(String, u8)> {
    // First, try to parse as a numeric index
    if let Ok(index) = pallet_id.parse::<u8>() {
        for pallet in pallets {
            if pallet.index == index {
                return Some((pallet.name.clone(), pallet.index));
            }
        }
    }

    // Otherwise, search by name (case-insensitive)
    let pallet_id_lower = pallet_id.to_lowercase();
    for pallet in pallets {
        if pallet.name.to_lowercase() == pallet_id_lower {
            return Some((pallet.name.clone(), pallet.index));
        }
    }

    None
}
