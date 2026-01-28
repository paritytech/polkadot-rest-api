//! Common types and utilities for coretime endpoints.
//!
//! This module provides shared error types and response types
//! used by coretime endpoints.

use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::Serialize;
use serde_json::json;
use thiserror::Error;

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug, Error)]
pub enum CoretimeError {
    // ========================================================================
    // Block/Client Errors
    // ========================================================================
    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] crate::utils::BlockIdParseError),

    #[error("{0}")]
    BlockResolveFailed(#[from] crate::utils::BlockResolveError),

    #[error("Invalid block hash format")]
    InvalidBlockHash,

    #[error("Failed to get client at block")]
    ClientAtBlockFailed(#[from] subxt::error::OnlineClientAtBlockError),

    // ========================================================================
    // Chain Type Errors
    // ========================================================================
    #[error("This endpoint is only available on coretime chains (chains with the Broker pallet)")]
    NotCoretimeChain,

    // ========================================================================
    // Pallet Errors
    // ========================================================================
    #[error("Broker pallet not found at this block")]
    BrokerPalletNotFound,

    #[error("Failed to fetch {pallet}::{entry} storage")]
    StorageFetchFailed {
        pallet: &'static str,
        entry: &'static str,
    },

    #[error("Failed to decode {pallet}::{entry} storage: {details}")]
    StorageDecodeFailed {
        pallet: &'static str,
        entry: &'static str,
        details: String,
    },

    #[error("Storage iteration error for {pallet}::{entry}: {details}")]
    StorageIterationError {
        pallet: &'static str,
        entry: &'static str,
        details: String,
    },
}

impl IntoResponse for CoretimeError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            // Block/Client errors
            CoretimeError::InvalidBlockParam(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            CoretimeError::BlockResolveFailed(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            CoretimeError::InvalidBlockHash => (StatusCode::BAD_REQUEST, self.to_string()),
            CoretimeError::ClientAtBlockFailed(err) => {
                if crate::utils::is_online_client_at_block_disconnected(err) {
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        format!("Service temporarily unavailable: {}", err),
                    )
                } else {
                    (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
                }
            }

            // Chain type errors
            CoretimeError::NotCoretimeChain => (StatusCode::BAD_REQUEST, self.to_string()),

            // Pallet errors
            CoretimeError::BrokerPalletNotFound => (StatusCode::NOT_FOUND, self.to_string()),
            CoretimeError::StorageFetchFailed { .. } => (StatusCode::NOT_FOUND, self.to_string()),
            CoretimeError::StorageDecodeFailed { .. } => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
            CoretimeError::StorageIterationError { .. } => {
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

// ============================================================================
// Query Parameters
// ============================================================================

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoretimeQueryParams {
    /// Block number or 0x-prefixed block hash to query at.
    /// If not provided, queries at the latest finalized block.
    pub at: Option<String>,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::response::IntoResponse;

    // ------------------------------------------------------------------------
    // Error message tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_coretime_error_not_coretime_chain_message() {
        let err = CoretimeError::NotCoretimeChain;
        assert_eq!(
            err.to_string(),
            "This endpoint is only available on coretime chains (chains with the Broker pallet)"
        );
    }

    #[test]
    fn test_coretime_error_broker_pallet_not_found_message() {
        let err = CoretimeError::BrokerPalletNotFound;
        assert_eq!(err.to_string(), "Broker pallet not found at this block");
    }

    #[test]
    fn test_coretime_error_storage_fetch_failed_message() {
        let err = CoretimeError::StorageFetchFailed {
            pallet: "Broker",
            entry: "Leases",
        };
        assert_eq!(err.to_string(), "Failed to fetch Broker::Leases storage");
    }

    #[test]
    fn test_coretime_error_storage_decode_failed_message() {
        let err = CoretimeError::StorageDecodeFailed {
            pallet: "Broker",
            entry: "Leases",
            details: "invalid data".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Failed to decode Broker::Leases storage: invalid data"
        );
    }

    #[test]
    fn test_coretime_error_storage_iteration_error_message() {
        let err = CoretimeError::StorageIterationError {
            pallet: "Broker",
            entry: "Workload",
            details: "connection lost".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Storage iteration error for Broker::Workload: connection lost"
        );
    }

    // ------------------------------------------------------------------------
    // HTTP Status code tests
    // ------------------------------------------------------------------------

    #[tokio::test]
    async fn test_coretime_error_not_coretime_chain_status() {
        let err = CoretimeError::NotCoretimeChain;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_coretime_error_broker_pallet_not_found_status() {
        let err = CoretimeError::BrokerPalletNotFound;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_coretime_error_storage_fetch_failed_status() {
        let err = CoretimeError::StorageFetchFailed {
            pallet: "Broker",
            entry: "Leases",
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_coretime_error_storage_decode_failed_status() {
        let err = CoretimeError::StorageDecodeFailed {
            pallet: "Broker",
            entry: "Leases",
            details: "invalid".to_string(),
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_coretime_error_storage_iteration_error_status() {
        let err = CoretimeError::StorageIterationError {
            pallet: "Broker",
            entry: "Workload",
            details: "error".to_string(),
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_coretime_error_response_body_format() {
        let err = CoretimeError::BrokerPalletNotFound;
        let response = err.into_response();

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_str = String::from_utf8(body.to_vec()).unwrap();

        // Should be JSON with "error" field
        assert!(body_str.contains("\"error\""));
        assert!(body_str.contains("Broker pallet not found"));
    }

    // ------------------------------------------------------------------------
    // AtResponse tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_at_response_serialization() {
        let at = AtResponse {
            hash: "0xabc123".to_string(),
            height: "12345".to_string(),
        };

        let json = serde_json::to_string(&at).unwrap();
        assert!(json.contains("\"hash\":\"0xabc123\""));
        assert!(json.contains("\"height\":\"12345\""));
    }

    #[test]
    fn test_at_response_clone() {
        let at = AtResponse {
            hash: "0xabc".to_string(),
            height: "100".to_string(),
        };

        let cloned = at.clone();
        assert_eq!(cloned.hash, "0xabc");
        assert_eq!(cloned.height, "100");
    }

    // ------------------------------------------------------------------------
    // CoretimeQueryParams tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_query_params_deserialize_with_at() {
        let json = r#"{"at": "12345"}"#;
        let params: CoretimeQueryParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.at, Some("12345".to_string()));
    }

    #[test]
    fn test_query_params_deserialize_with_at_hash() {
        let json = r#"{"at": "0xabc123"}"#;
        let params: CoretimeQueryParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.at, Some("0xabc123".to_string()));
    }

    #[test]
    fn test_query_params_deserialize_without_at() {
        let json = r#"{}"#;
        let params: CoretimeQueryParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.at, None);
    }

    #[test]
    fn test_query_params_deserialize_null_at() {
        let json = r#"{"at": null}"#;
        let params: CoretimeQueryParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.at, None);
    }
}
