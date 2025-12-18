pub mod block;
pub mod extrinsic;
pub mod fee;
pub mod format;
pub mod hash;

pub use block::{BlockId, BlockIdParseError, BlockResolveError, ResolvedBlock, resolve_block};
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

/// Check if an RPC error indicates the connection was lost and reconnection is in progress.
///
/// When using the reconnecting RPC client, this error indicates temporary unavailability
/// while the client attempts to re-establish the WebSocket connection.
pub fn is_disconnected_error(err: &subxt_rpcs::Error) -> bool {
    matches!(err, subxt_rpcs::Error::DisconnectedWillReconnect(_))
}

/// Check if an OnlineClientAtBlockError contains a disconnection error.
///
/// The OnlineClientAtBlockError may wrap an RPC error (e.g., in CannotGetBlockHash)
/// that indicates the connection was lost. This helper extracts and checks that inner error.
pub fn is_online_client_at_block_disconnected(
    err: &subxt_historic::error::OnlineClientAtBlockError,
) -> bool {
    use subxt_historic::error::OnlineClientAtBlockError;

    match err {
        OnlineClientAtBlockError::CannotGetBlockHash { reason, .. } => {
            is_disconnected_error(reason)
        }
        // Other variants don't contain RPC errors that could be disconnection errors
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
