//! Fee calculation utilities for extrinsics.
//!
//! This module provides:
//! - `calc_partial_fee`: Core fee calculation using Perbill for precision
//! - `QueryFeeDetailsCache`: Tracks whether `payment_queryFeeDetails` is available per spec_version
//! - `parse_fee_details` / `extract_estimated_weight`: RPC response parsing utilities

use config::ChainFeeConfigs;
use serde_json::Value;
use sp_runtime::Perbill;
use std::collections::HashMap;
use std::sync::RwLock;
use thiserror::Error;

// ================================================================================================
// Error Types
// ================================================================================================

/// Error type for fee calculation operations
#[derive(Debug, Error)]
pub enum FeeCalcError {
    #[error("Failed to parse {field} as u128: {value}")]
    ParseError { field: &'static str, value: String },
}

#[derive(Debug, Error)]
pub enum FeeServiceError {
    #[error("RPC error: {0}")]
    RpcError(#[from] subxt_rpcs::Error),

    #[error("Fee calculation error: {0}")]
    CalcError(#[from] FeeCalcError),

    #[error("Missing required fee data: {0}")]
    MissingData(String),
}

// ================================================================================================
// Runtime Dispatch Info Types
// ================================================================================================

/// Raw runtime dispatch info from TransactionPaymentApi_query_info
///
/// This is the decoded response from calling the `TransactionPaymentApi_query_info`
/// runtime API via `state_call`. It contains fee estimation data for an extrinsic.
#[derive(Debug, Clone)]
pub struct RuntimeDispatchInfoRaw {
    /// Weight consumed by the extrinsic
    pub weight: WeightRaw,
    /// Dispatch class (Normal, Operational, or Mandatory)
    pub class: String,
    /// Partial fee (pre-dispatch estimation)
    pub partial_fee: u128,
}

impl RuntimeDispatchInfoRaw {
    /// Convert to JSON Value matching the format of payment_queryInfo RPC response
    pub fn to_json(&self) -> Value {
        serde_json::json!({
            "weight": self.weight.to_json(),
            "class": self.class,
            "partialFee": self.partial_fee.to_string()
        })
    }
}

/// Weight format - can be either V1 (single u64) or V2 (ref_time + proof_size)
///
/// Older runtimes used a single u64 for weight (V1), while modern runtimes
/// use a two-dimensional weight with ref_time and proof_size (V2).
#[derive(Debug, Clone)]
pub enum WeightRaw {
    /// Legacy weight format (pre-V2): single u64 value
    V1(u64),
    /// Modern weight format (V2): ref_time and proof_size components
    V2 { ref_time: u64, proof_size: u64 },
}

impl WeightRaw {
    /// Convert to JSON Value for API response
    pub fn to_json(&self) -> Value {
        match self {
            WeightRaw::V1(w) => serde_json::json!(w.to_string()),
            WeightRaw::V2 {
                ref_time,
                proof_size,
            } => serde_json::json!({
                "refTime": ref_time.to_string(),
                "proofSize": proof_size.to_string()
            }),
        }
    }

    /// Get the ref_time value (or the single weight value for V1)
    pub fn ref_time(&self) -> u64 {
        match self {
            WeightRaw::V1(w) => *w,
            WeightRaw::V2 { ref_time, .. } => *ref_time,
        }
    }
}

/// Convert dispatch class byte to string representation
///
/// The dispatch class is encoded as a single byte in the SCALE-encoded
/// RuntimeDispatchInfo response from the runtime API.
pub fn dispatch_class_from_u8(class: u8) -> String {
    match class {
        0 => "Normal".to_string(),
        1 => "Operational".to_string(),
        2 => "Mandatory".to_string(),
        _ => "Unknown".to_string(),
    }
}

// ================================================================================================
// Core Fee Calculation
// ================================================================================================

/// Calculate the partial fee for an extrinsic.
///
/// The partial fee is the total fee minus any tip. It uses the following formula:
///
/// ```text
/// partial_fee = base_fee + len_fee + ((adjusted_weight_fee/estimated_weight)*actual_weight)
/// ```
///
/// Where:
/// - `base_fee` is a fixed base fee to include some transaction in a block. It accounts
///   for the work needed to verify the signature and the computing work common to any tx.
/// - `len_fee` is a fee paid based on the size (length in bytes) of the transaction.
/// - `adjusted_weight_fee` is `estimated_weight * targeted_fee_adjustment`, where
///   `targeted_fee_adjustment` is an opaque internal value based on network load.
/// - `estimated_weight` is the "pre-dispatch" weight of the transaction.
/// - `actual_weight` is the weight from the `ExtrinsicSuccess` event after execution.
///
/// # Arguments
///
/// * `base_fee` - The base fee as a string (parsed as u128)
/// * `len_fee` - The length-based fee as a string (parsed as u128)
/// * `adjusted_weight_fee` - The adjusted weight fee as a string (parsed as u128)
/// * `estimated_weight` - The pre-dispatch estimated weight as a string (parsed as u128)
/// * `actual_weight` - The actual weight from ExtrinsicSuccess event as a string (parsed as u128)
///
/// # Returns
///
/// The calculated partial fee as a string, or an error if parsing fails.
pub fn calc_partial_fee(
    base_fee: &str,
    len_fee: &str,
    adjusted_weight_fee: &str,
    estimated_weight: &str,
    actual_weight: &str,
) -> Result<String, FeeCalcError> {
    let base_fee: u128 = base_fee.parse().map_err(|_| FeeCalcError::ParseError {
        field: "base_fee",
        value: base_fee.to_string(),
    })?;
    let len_fee: u128 = len_fee.parse().map_err(|_| FeeCalcError::ParseError {
        field: "len_fee",
        value: len_fee.to_string(),
    })?;
    let adjusted_weight_fee: u128 =
        adjusted_weight_fee
            .parse()
            .map_err(|_| FeeCalcError::ParseError {
                field: "adjusted_weight_fee",
                value: adjusted_weight_fee.to_string(),
            })?;
    let estimated_weight: u128 =
        estimated_weight
            .parse()
            .map_err(|_| FeeCalcError::ParseError {
                field: "estimated_weight",
                value: estimated_weight.to_string(),
            })?;
    let actual_weight: u128 = actual_weight
        .parse()
        .map_err(|_| FeeCalcError::ParseError {
            field: "actual_weight",
            value: actual_weight.to_string(),
        })?;

    let partial_fee = calc_partial_fee_raw(
        base_fee,
        len_fee,
        adjusted_weight_fee,
        estimated_weight,
        actual_weight,
    );

    Ok(partial_fee.to_string())
}

/// Calculate the partial fee with raw u128 values.
///
/// This is the internal calculation function that uses `Perbill::from_rational`
/// for precision when adjusting the weight fee.
pub fn calc_partial_fee_raw(
    base_fee: u128,
    len_fee: u128,
    adjusted_weight_fee: u128,
    estimated_weight: u128,
    actual_weight: u128,
) -> u128 {
    // Calculate new adjusted_weight_fee, trying to maintain precision.
    // The ratio is estimated_weight/actual_weight, which adjusts the fee
    // based on how the actual execution weight differed from the estimate.
    let adjusted_weight_fee =
        Perbill::from_rational(estimated_weight, actual_weight) * adjusted_weight_fee;

    // Add the fee components together to get the partial/inclusion fee
    base_fee
        .saturating_add(len_fee)
        .saturating_add(adjusted_weight_fee)
}

// ================================================================================================
// QueryFeeDetails Cache
// ================================================================================================

/// Cache for tracking whether `payment_queryFeeDetails` is available at different spec versions.
///
/// The cache uses a three-tier approach:
/// 1. Static config check: Uses chain fee configs to determine availability without RPC
/// 2. Runtime cache: Caches results from actual RPC calls
/// 3. Default: Falls back to trying the RPC call if unknown
pub struct QueryFeeDetailsCache {
    /// Cached results: spec_version -> is_available
    cache: RwLock<HashMap<u32, bool>>,
    /// Chain fee configurations for static lookup
    fee_configs: ChainFeeConfigs,
}

impl QueryFeeDetailsCache {
    pub fn new() -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            fee_configs: ChainFeeConfigs::default(),
        }
    }

    /// Check if queryFeeDetails is known to be available for a given spec version.
    ///
    /// Returns:
    /// - `Some(true)` if known to be available
    /// - `Some(false)` if known to be unavailable
    /// - `None` if unknown and needs to be discovered via RPC
    pub fn is_available(&self, spec_name: &str, spec_version: u32) -> Option<bool> {
        // First, check the static config
        if let Some(config) = self.fee_configs.get(spec_name)
            && let Some(status) = config.query_fee_details_status(spec_version)
        {
            return Some(status);
        }

        // Check runtime cache
        let cache = self.cache.read().ok()?;
        cache.get(&spec_version).copied()
    }

    /// Record the result of a queryFeeDetails availability check
    pub fn set_available(&self, spec_version: u32, available: bool) {
        if let Ok(mut cache) = self.cache.write() {
            cache.insert(spec_version, available);
        }
    }

    /// Check if fee calculation is supported for a given chain and spec version
    pub fn supports_fee_calculation(&self, spec_name: &str, spec_version: u32) -> bool {
        if let Some(config) = self.fee_configs.get(spec_name) {
            config.supports_fee_calculation(spec_version)
        } else {
            // For unknown chains, assume fee calculation is supported
            true
        }
    }
}

impl Default for QueryFeeDetailsCache {
    fn default() -> Self {
        Self::new()
    }
}

// ================================================================================================
// Fee Details Parsing
// ================================================================================================

/// Fee details extracted from `payment_queryFeeDetails` RPC response
#[derive(Debug, Clone)]
pub struct FeeDetails {
    /// Base fee for transaction inclusion
    pub base_fee: String,
    /// Fee based on transaction length in bytes
    pub len_fee: String,
    /// Adjusted weight fee (estimated_weight * fee_adjustment)
    pub adjusted_weight_fee: String,
}

/// Parse fee details from the JSON response of `payment_queryFeeDetails`
///
/// Expected response format:
/// ```json
/// {
///   "inclusionFee": {
///     "baseFee": "...",
///     "lenFee": "...",
///     "adjustedWeightFee": "..."
///   }
/// }
/// ```
pub fn parse_fee_details(response: &Value) -> Option<FeeDetails> {
    let inclusion_fee = response.get("inclusionFee")?;

    // inclusionFee can be null if the transaction doesn't pay fees
    if inclusion_fee.is_null() {
        return None;
    }

    let base_fee = extract_fee_value(inclusion_fee.get("baseFee")?)?;
    let len_fee = extract_fee_value(inclusion_fee.get("lenFee")?)?;
    let adjusted_weight_fee = extract_fee_value(inclusion_fee.get("adjustedWeightFee")?)?;

    Some(FeeDetails {
        base_fee,
        len_fee,
        adjusted_weight_fee,
    })
}

/// Extract a fee value from JSON, handling different formats (number, hex string, decimal string)
fn extract_fee_value(value: &Value) -> Option<String> {
    match value {
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => {
            if s.starts_with("0x") {
                // Convert hex to decimal
                u128::from_str_radix(s.trim_start_matches("0x"), 16)
                    .map(|n| n.to_string())
                    .ok()
            } else {
                Some(s.clone())
            }
        }
        _ => None,
    }
}

/// Extract the estimated weight from `payment_queryInfo` response
///
/// Expected response format:
/// ```json
/// {
///   "weight": { "refTime": "...", "proofSize": "..." }  // modern
///   // OR
///   "weight": "..."  // legacy (single number)
/// }
/// ```
pub fn extract_estimated_weight(query_info: &Value) -> Option<String> {
    let weight = query_info.get("weight")?;

    match weight {
        // Modern weight format: { refTime, proofSize }
        Value::Object(obj) => {
            let ref_time = obj.get("refTime")?;
            extract_fee_value(ref_time)
        }
        // Legacy weight format: single number
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => {
            if s.starts_with("0x") {
                u128::from_str_radix(s.trim_start_matches("0x"), 16)
                    .map(|n| n.to_string())
                    .ok()
            } else {
                Some(s.clone())
            }
        }
        _ => None,
    }
}

/// Calculate the accurate partial fee using fee details and actual weight.
///
/// This uses the formula:
/// ```text
/// partial_fee = base_fee + len_fee + ((adjusted_weight_fee/estimated_weight)*actual_weight)
/// ```
///
/// Returns the calculated partial fee as a string.
pub fn calculate_accurate_fee(
    fee_details: &FeeDetails,
    estimated_weight: &str,
    actual_weight: &str,
) -> Result<String, FeeServiceError> {
    calc_partial_fee(
        &fee_details.base_fee,
        &fee_details.len_fee,
        &fee_details.adjusted_weight_fee,
        estimated_weight,
        actual_weight,
    )
    .map_err(FeeServiceError::from)
}

// ================================================================================================
// Tests
// ================================================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // --- calc_partial_fee tests ---

    #[test]
    fn test_calc_partial_fee_basic() {
        // When estimated_weight equals actual_weight, the adjusted fee should remain unchanged
        let result = calc_partial_fee("100", "50", "200", "1000", "1000").unwrap();
        // 100 + 50 + (200 * 1000/1000) = 350
        assert_eq!(result, "350");
    }

    #[test]
    fn test_calc_partial_fee_lower_actual_weight() {
        // When actual_weight is lower than estimated, the fee should be adjusted down
        let result = calc_partial_fee("100", "50", "200", "500", "1000").unwrap();
        // 100 + 50 + (200 * 500/1000) = 100 + 50 + 100 = 250
        assert_eq!(result, "250");
    }

    #[test]
    fn test_calc_partial_fee_higher_actual_weight() {
        // When actual_weight is higher than estimated, the adjustment is capped at 1 (Perbill max)
        let result = calc_partial_fee("100", "50", "200", "2000", "1000").unwrap();
        // The ratio 2000/1000 = 2, but Perbill caps at 1, so it's just the full adjusted_weight_fee
        // 100 + 50 + 200 = 350
        assert_eq!(result, "350");
    }

    #[test]
    fn test_calc_partial_fee_zero_base_and_len() {
        let result = calc_partial_fee("0", "0", "1000", "500", "1000").unwrap();
        // 0 + 0 + (1000 * 500/1000) = 500
        assert_eq!(result, "500");
    }

    #[test]
    fn test_calc_partial_fee_large_values() {
        // Test with realistic large values
        let result = calc_partial_fee(
            "124000000",     // base_fee: 124_000_000
            "1500000000",    // len_fee: 1_500_000_000
            "8000000000000", // adjusted_weight_fee: 8_000_000_000_000
            "210000000000",  // estimated_weight: 210_000_000_000
            "220000000000",  // actual_weight: 220_000_000_000
        )
        .unwrap();
        // The ratio is 210/220 ≈ 0.9545454545
        // adjusted = 8_000_000_000_000 * 0.9545454545 ≈ 7_636_363_636_363
        // total = 124_000_000 + 1_500_000_000 + 7_636_363_636_363 ≈ 7_637_987_636_363
        let partial_fee: u128 = result.parse().unwrap();
        assert!(partial_fee > 7_600_000_000_000);
        assert!(partial_fee < 7_700_000_000_000);
    }

    #[test]
    fn test_calc_partial_fee_raw_precision() {
        // Test that Perbill maintains precision
        let result = calc_partial_fee_raw(
            0,
            0,
            1_000_000_000_000, // 1 trillion
            333_333_333,       // estimated
            1_000_000_000,     // actual
        );
        // Ratio is 333_333_333 / 1_000_000_000 = 0.333333333
        // Result should be roughly 333_333_333_000
        assert!(result > 333_000_000_000);
        assert!(result < 334_000_000_000);
    }

    #[test]
    fn test_calc_partial_fee_invalid_input() {
        let result = calc_partial_fee("not_a_number", "50", "200", "1000", "1000");
        assert!(result.is_err());
        match result {
            Err(FeeCalcError::ParseError { field, .. }) => {
                assert_eq!(field, "base_fee");
            }
            _ => panic!("Expected ParseError"),
        }
    }

    #[test]
    fn test_calc_partial_fee_saturating_add() {
        // Test that saturating_add prevents overflow
        let result = calc_partial_fee_raw(u128::MAX - 100, 100, 100, 1000, 1000);
        // Should saturate to MAX instead of overflowing
        assert_eq!(result, u128::MAX);
    }

    // --- QueryFeeDetailsCache tests ---

    #[test]
    fn test_cache_static_lookup_polkadot() {
        let cache = QueryFeeDetailsCache::new();

        // Polkadot: queryFeeDetails unavailable at spec 27, available at spec 28
        assert_eq!(cache.is_available("polkadot", 27), Some(false));
        assert_eq!(cache.is_available("polkadot", 28), Some(true));
        assert_eq!(cache.is_available("polkadot", 100), Some(true));
    }

    #[test]
    fn test_cache_static_lookup_kusama() {
        let cache = QueryFeeDetailsCache::new();

        // Kusama: queryFeeDetails unavailable at spec 2027, available at spec 2028
        assert_eq!(cache.is_available("kusama", 2027), Some(false));
        assert_eq!(cache.is_available("kusama", 2028), Some(true));
    }

    #[test]
    fn test_cache_static_lookup_asset_hub() {
        let cache = QueryFeeDetailsCache::new();

        // Asset hub chains have unknown queryFeeDetails status (null in config)
        assert_eq!(cache.is_available("asset-hub-polkadot", 1000), None);
        assert_eq!(cache.is_available("statemint", 1000), None);
    }

    #[test]
    fn test_cache_runtime_cache() {
        let cache = QueryFeeDetailsCache::new();

        // Unknown chain should return None
        assert_eq!(cache.is_available("unknown-chain", 100), None);

        // Set availability in cache
        cache.set_available(100, true);

        // For asset-hub (which has null in config), the runtime cache should work
        cache.set_available(1000, false);
    }

    #[test]
    fn test_supports_fee_calculation() {
        let cache = QueryFeeDetailsCache::new();

        // Polkadot supports fee calculation from spec 0
        assert!(cache.supports_fee_calculation("polkadot", 0));
        assert!(cache.supports_fee_calculation("polkadot", 100));

        // Kusama supports from spec 1058
        assert!(!cache.supports_fee_calculation("kusama", 1057));
        assert!(cache.supports_fee_calculation("kusama", 1058));

        // Unknown chains default to supported
        assert!(cache.supports_fee_calculation("unknown-chain", 1));
    }

    // --- Fee details parsing tests ---

    #[test]
    fn test_parse_fee_details() {
        let response = json!({
            "inclusionFee": {
                "baseFee": "124000000",
                "lenFee": "1500000000",
                "adjustedWeightFee": "8000000000000"
            }
        });

        let details = parse_fee_details(&response).unwrap();
        assert_eq!(details.base_fee, "124000000");
        assert_eq!(details.len_fee, "1500000000");
        assert_eq!(details.adjusted_weight_fee, "8000000000000");
    }

    #[test]
    fn test_parse_fee_details_hex() {
        let response = json!({
            "inclusionFee": {
                "baseFee": "0x7643c00",
                "lenFee": "0x59682f00",
                "adjustedWeightFee": "0x746a528800"
            }
        });

        let details = parse_fee_details(&response).unwrap();
        // 0x7643c00 = 124009472
        assert_eq!(details.base_fee, "124009472");
        // 0x59682f00 = 1500000000
        assert_eq!(details.len_fee, "1500000000");
        // 0x746a528800 = 500000000000
        assert_eq!(details.adjusted_weight_fee, "500000000000");
    }

    #[test]
    fn test_parse_fee_details_null_inclusion_fee() {
        let response = json!({
            "inclusionFee": null
        });

        let details = parse_fee_details(&response);
        assert!(details.is_none());
    }

    #[test]
    fn test_extract_estimated_weight_modern() {
        let query_info = json!({
            "weight": {
                "refTime": "210000000000",
                "proofSize": "3593"
            }
        });

        let weight = extract_estimated_weight(&query_info).unwrap();
        assert_eq!(weight, "210000000000");
    }

    #[test]
    fn test_extract_estimated_weight_legacy() {
        let query_info = json!({
            "weight": 150000000
        });

        let weight = extract_estimated_weight(&query_info).unwrap();
        assert_eq!(weight, "150000000");
    }

    #[test]
    fn test_calculate_accurate_fee() {
        let fee_details = FeeDetails {
            base_fee: "124000000".to_string(),
            len_fee: "1500000000".to_string(),
            adjusted_weight_fee: "8000000000000".to_string(),
        };

        // When estimated == actual, fee should be base + len + adjusted
        let fee = calculate_accurate_fee(&fee_details, "210000000000", "210000000000").unwrap();
        // 124000000 + 1500000000 + 8000000000000 = 8001624000000
        assert_eq!(fee, "8001624000000");
    }

    #[test]
    fn test_calculate_accurate_fee_with_weight_adjustment() {
        let fee_details = FeeDetails {
            base_fee: "100".to_string(),
            len_fee: "50".to_string(),
            adjusted_weight_fee: "1000".to_string(),
        };

        // When estimated weight is half of actual, adjusted fee should be halved
        let fee = calculate_accurate_fee(&fee_details, "500", "1000").unwrap();
        // 100 + 50 + (1000 * 500/1000) = 100 + 50 + 500 = 650
        assert_eq!(fee, "650");
    }
}
