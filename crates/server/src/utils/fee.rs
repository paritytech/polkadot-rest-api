use sp_runtime::Perbill;
use thiserror::Error;

/// Error type for fee calculation operations
#[derive(Debug, Error)]
pub enum FeeCalcError {
    #[error("Failed to parse {field} as u128: {value}")]
    ParseError { field: &'static str, value: String },
}

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
