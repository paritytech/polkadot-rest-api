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
