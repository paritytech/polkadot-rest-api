pub mod block;
pub mod capabilities;
pub mod extrinsic;
pub mod fee;
pub mod format;
pub mod hash;
pub mod rc_block;

pub use block::{
    BlockId, BlockIdParseError, BlockResolveError, ResolvedBlock, fetch_block_timestamp,
    get_block_number_from_hash_with_rpc, resolve_block, resolve_block_with_rpc,
};
pub use extrinsic::{
    EraInfo, decode_era_from_bytes, extract_era_from_extrinsic_bytes, parse_era_info,
};
pub use fee::{
    FeeCalcError, FeeDetails, FeeServiceError, QueryFeeDetailsCache, RuntimeDispatchInfoRaw,
    WeightRaw, calc_partial_fee, calc_partial_fee_raw, calculate_accurate_fee,
    dispatch_class_from_u8, extract_estimated_weight, parse_fee_details,
};
pub use format::{decode_address_to_ss58, hex_with_prefix, lowercase_first_char};
pub use hash::{HashError, compute_block_hash_from_header_json, parse_block_number_from_json};
pub use rc_block::{
    AhBlockInfo, RcBlockError, RcClientAtBlock, extract_block_number_from_header,
    extract_bytes_from_json, find_ah_blocks_in_rc_block, find_ah_blocks_in_rc_block_at,
};

/// Check if an RPC error indicates the connection was lost and reconnection is in progress.
///
/// When using the reconnecting RPC client, this error indicates temporary unavailability
/// while the client attempts to re-establish the WebSocket connection.
pub fn is_disconnected_error(err: &subxt_rpcs::Error) -> bool {
    matches!(err, subxt_rpcs::Error::DisconnectedWillReconnect(_))
}

/// Check if a BackendError indicates the connection was lost and reconnection is in progress.
///
/// This is the subxt 0.50+ variant that wraps RPC errors in BackendError.
pub fn is_backend_disconnected_error(err: &subxt::error::BackendError) -> bool {
    err.is_disconnected_will_reconnect()
}

/// Check if an OnlineClientAtBlockError contains a disconnection error.
///
/// The OnlineClientAtBlockError may wrap a BackendError (e.g., in CannotGetBlockHash)
/// that indicates the connection was lost. This helper extracts and checks that inner error.
pub fn is_online_client_at_block_disconnected(
    err: &subxt::error::OnlineClientAtBlockError,
) -> bool {
    use subxt::error::OnlineClientAtBlockError;

    match err {
        OnlineClientAtBlockError::CannotGetBlockHash { reason, .. } => {
            is_backend_disconnected_error(reason)
        }
        OnlineClientAtBlockError::CannotGetCurrentBlock { reason } => {
            is_backend_disconnected_error(reason)
        }
        OnlineClientAtBlockError::CannotGetBlockHeader { reason, .. } => {
            is_backend_disconnected_error(reason)
        }
        OnlineClientAtBlockError::CannotGetSpecVersion { reason, .. } => {
            is_backend_disconnected_error(reason)
        }
        // Other variants don't contain BackendErrors that could be disconnection errors
        _ => false,
    }
}

/// Check if an RPC error is a request timeout.
///
/// Timeout errors occur when an RPC request takes longer than the configured
/// request_timeout (default: 30s). This typically happens when the node is
/// unresponsive or the connection is degraded.
pub fn is_timeout_error(err: &subxt_rpcs::Error) -> bool {
    match err {
        subxt_rpcs::Error::Client(inner) => {
            // jsonrpsee returns "Request timeout" for timeout errors
            inner.to_string().contains("Request timeout")
        }
        _ => false,
    }
}

/// Convert an RPC error to an appropriate HTTP status code and message.
///
/// This centralizes the logic for handling different RPC error types:
/// - Timeout errors → 504 Gateway Timeout
/// - Disconnection errors → 503 Service Unavailable
/// - Other errors → 500 Internal Server Error
pub fn rpc_error_to_status(err: &subxt_rpcs::Error) -> (axum::http::StatusCode, String) {
    use axum::http::StatusCode;

    if is_timeout_error(err) {
        (
            StatusCode::GATEWAY_TIMEOUT,
            "Request timed out while waiting for node response".to_string(),
        )
    } else if is_disconnected_error(err) {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("Service temporarily unavailable: {}", err),
        )
    } else {
        (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
    }
}

#[cfg(test)]
mod rpc_error_tests {
    use super::*;
    use axum::http::StatusCode;

    /// Helper to create a timeout error (simulates jsonrpsee RequestTimeout)
    fn make_timeout_error() -> subxt_rpcs::Error {
        subxt_rpcs::Error::Client(Box::new(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "Request timeout",
        )))
    }

    /// Helper to create a disconnection error
    fn make_disconnected_error() -> subxt_rpcs::Error {
        subxt_rpcs::Error::DisconnectedWillReconnect("Connection lost".to_string())
    }

    /// Helper to create a generic RPC error
    fn make_generic_error() -> subxt_rpcs::Error {
        subxt_rpcs::Error::Client(Box::new(std::io::Error::other("Some other error")))
    }

    #[test]
    fn test_is_disconnected_error_true() {
        let err = make_disconnected_error();
        assert!(is_disconnected_error(&err));
    }

    #[test]
    fn test_is_disconnected_error_false_for_timeout() {
        let err = make_timeout_error();
        assert!(!is_disconnected_error(&err));
    }

    #[test]
    fn test_is_disconnected_error_false_for_generic() {
        let err = make_generic_error();
        assert!(!is_disconnected_error(&err));
    }

    #[test]
    fn test_is_timeout_error_true() {
        let err = make_timeout_error();
        assert!(is_timeout_error(&err));
    }

    #[test]
    fn test_is_timeout_error_false_for_disconnected() {
        let err = make_disconnected_error();
        assert!(!is_timeout_error(&err));
    }

    #[test]
    fn test_is_timeout_error_false_for_generic() {
        let err = make_generic_error();
        assert!(!is_timeout_error(&err));
    }

    #[test]
    fn test_rpc_error_to_status_timeout() {
        let err = make_timeout_error();
        let (status, message) = rpc_error_to_status(&err);
        assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
        assert!(message.contains("timed out"));
    }

    #[test]
    fn test_rpc_error_to_status_disconnected() {
        let err = make_disconnected_error();
        let (status, message) = rpc_error_to_status(&err);
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(message.contains("temporarily unavailable"));
    }

    #[test]
    fn test_rpc_error_to_status_generic() {
        let err = make_generic_error();
        let (status, _message) = rpc_error_to_status(&err);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }
}
