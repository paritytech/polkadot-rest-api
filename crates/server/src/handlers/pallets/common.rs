//! Common types and utilities shared across pallet endpoints.
//!
//! This module provides shared error types and response types
//! used by the assets endpoint.

use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::Serialize;
use serde_json::json;
use thiserror::Error;

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug, Error)]
pub enum PalletError {
    // ========================================================================
    // Block/Client Errors
    // ========================================================================
    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] crate::utils::BlockIdParseError),

    #[error("Block resolution failed")]
    BlockResolveFailed(#[from] crate::utils::BlockResolveError),

    #[error("Failed to get client at block")]
    ClientAtBlockFailed(#[from] subxt::error::OnlineClientAtBlockError),

    #[error("Bad staking block: {0}")]
    BadStakingBlock(String),

    // ========================================================================
    // Relay Chain Errors
    // ========================================================================
    #[error("Relay chain connection not configured")]
    RelayChainNotConfigured,

    #[error("RC block error: {0}")]
    RcBlockError(#[from] crate::utils::rc_block::RcBlockError),

    #[error("useRcBlock is only supported for Asset Hub chains")]
    UseRcBlockNotSupported,

    #[error("at parameter is required when useRcBlock=true")]
    AtParameterRequired,

    // ========================================================================
    // Storage Fetch Errors
    // ========================================================================
    #[error("Failed to fetch {pallet}::{entry} storage")]
    StorageFetchFailed {
        pallet: &'static str,
        entry: &'static str,
    },

    #[error("Fetch entry of {pallet}::{entry} storage failed with {error}")]
    StorageEntryFetchFailed {
        pallet: &'static str,
        entry: &'static str,
        error: String,
    },

    #[error("Failed to decode {pallet}::{entry} storage")]
    StorageDecodeFailed {
        pallet: &'static str,
        entry: &'static str,
    },

    #[error("Pallet not found: {0}")]
    PalletNotFound(String),

    #[error("Asset not found: {0}")]
    AssetNotFound(String),

    // ========================================================================
    // Metadata/Constant Errors
    // ========================================================================
    #[error("Constant {pallet}::{constant} not found in metadata")]
    ConstantNotFound {
        pallet: &'static str,
        constant: &'static str,
    },

    // ========================================================================
    // Staking-Specific Errors
    // ========================================================================
    #[error("Chain '{0}' is not supported for staking progress queries")]
    UnsupportedChainForStaking(String),

    #[error("Active era not found at this block")]
    ActiveEraNotFound,

    #[error("Era start session index not found in BondedEras for active era")]
    EraStartSessionNotFound,

    // ========================================================================
    // Timestamp Errors
    // ========================================================================
    #[error("Failed to fetch timestamp from Timestamp::Now storage")]
    TimestampFetchFailed,

    #[error("Failed to parse timestamp value")]
    TimestampParseFailed,
}

impl IntoResponse for PalletError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            // Block/Client errors
            PalletError::InvalidBlockParam(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            PalletError::BlockResolveFailed(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            PalletError::BadStakingBlock(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            PalletError::ClientAtBlockFailed(err) => {
                if crate::utils::is_online_client_at_block_disconnected(err) {
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        format!("Service temporarily unavailable: {}", err),
                    )
                } else {
                    (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
                }
            }

            // Relay chain errors
            PalletError::RelayChainNotConfigured => (StatusCode::BAD_REQUEST, self.to_string()),
            PalletError::RcBlockError(_) => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
            PalletError::UseRcBlockNotSupported => (StatusCode::BAD_REQUEST, self.to_string()),
            PalletError::AtParameterRequired => (StatusCode::BAD_REQUEST, self.to_string()),

            // Storage errors - NOT_FOUND for missing data, INTERNAL_SERVER_ERROR for decode failures
            PalletError::StorageFetchFailed { .. } => (StatusCode::NOT_FOUND, self.to_string()),
            PalletError::StorageEntryFetchFailed { .. } => {
                (StatusCode::NOT_FOUND, self.to_string())
            }
            PalletError::StorageDecodeFailed { .. } => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            PalletError::PalletNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),
            PalletError::AssetNotFound(_) => (StatusCode::NOT_FOUND, self.to_string()),

            // Metadata errors
            PalletError::ConstantNotFound { .. } => (StatusCode::NOT_FOUND, self.to_string()),

            // Staking-specific errors
            PalletError::UnsupportedChainForStaking(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            PalletError::ActiveEraNotFound => (StatusCode::NOT_FOUND, self.to_string()),
            PalletError::EraStartSessionNotFound => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }

            // Timestamp errors
            PalletError::TimestampFetchFailed => (StatusCode::NOT_FOUND, self.to_string()),
            PalletError::TimestampParseFailed => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

// ============================================================================
// Response Types
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct AtResponse {
    pub hash: String,
    pub height: String,
}

/// Formats a 32-byte account ID to SS58 format.
pub fn format_account_id(account: &[u8; 32], ss58_prefix: u16) -> String {
    use sp_core::crypto::Ss58Codec;
    sp_core::sr25519::Public::from_raw(*account).to_ss58check_with_version(ss58_prefix.into())
}
