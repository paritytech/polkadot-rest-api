//! Utility functions for account-related handlers.

use scale_value::{Value, ValueDef};
use sp_core::crypto::AccountId32;
use std::str::FromStr;

use super::types::AssetBalancesError;

mod assets;
mod pool_assets;
mod timestamp;

pub use assets::{query_all_assets_id, query_assets};
pub use pool_assets::{query_all_pool_assets_id, query_pool_assets};
pub use timestamp::fetch_timestamp;
// ================================================================================================
// Address Validation
// ================================================================================================

/// Validate and parse account address (supports SS58 and hex formats)
pub fn validate_and_parse_address(addr: &str) -> Result<AccountId32, AssetBalancesError> {
    // Try SS58 format first (any network prefix)
    if let Ok(account) = AccountId32::from_str(addr) {
        return Ok(account);
    }

    // Try hex format (0x-prefixed, 32 bytes)
    if addr.starts_with("0x") && addr.len() == 66 {
        // 0x + 64 hex chars = 32 bytes
        if let Ok(bytes) = hex::decode(&addr[2..]) {
            if bytes.len() == 32 {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                return Ok(AccountId32::from(arr));
            }
        }
    }

    Err(AssetBalancesError::InvalidAddress(addr.to_string()))
}


/// Extract u128 field from named fields
pub fn extract_u128_field(fields: &[(String, Value<()>)], field_name: &str) -> Option<u128> {
    fields
        .iter()
        .find(|(name, _)| name == field_name)
        .and_then(|(_, value)| match &value.value {
            ValueDef::Primitive(scale_value::Primitive::U128(val)) => Some(*val),
            _ => None,
        })
}

/// Extract boolean field from named fields
pub fn extract_bool_field(fields: &[(String, Value<()>)], field_name: &str) -> Option<bool> {
    fields
        .iter()
        .find(|(name, _)| name == field_name)
        .and_then(|(_, value)| match &value.value {
            ValueDef::Primitive(scale_value::Primitive::Bool(val)) => Some(*val),
            _ => None,
        })
}

/// Extract isSufficient from reason enum
pub fn extract_is_sufficient_from_reason(reason_value: &Value<()>) -> bool {
    match &reason_value.value {
        ValueDef::Variant(variant) => {
            // Check if variant name is "Sufficient" or "isSufficient"
            variant.name == "Sufficient" || variant.name == "isSufficient"
        }
        _ => false,
    }
}

// ================================================================================================
// Tests
// ================================================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_validation_hex() {
        // Alice's address in hex
        let addr = "0xd43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d";
        assert!(validate_and_parse_address(addr).is_ok());
    }

    #[test]
    fn test_address_validation_ss58() {
        // Alice's address in SS58 (Polkadot prefix)
        let addr = "15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5";
        assert!(validate_and_parse_address(addr).is_ok());
    }

    #[test]
    fn test_address_validation_invalid() {
        let addr = "invalid-address";
        assert!(validate_and_parse_address(addr).is_err());
    }

    #[test]
    fn test_address_validation_short_hex() {
        let addr = "0x1234"; // Too short
        assert!(validate_and_parse_address(addr).is_err());
    }
}
