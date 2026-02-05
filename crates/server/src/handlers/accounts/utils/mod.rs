//! Utility functions for account-related handlers.

use sp_core::crypto::AccountId32;
use std::str::FromStr;

// ================================================================================================
// Address Validation Error
// ================================================================================================

/// Error type for address validation failures.
/// This is a standalone error type that can be converted to handler-specific errors via #[from].
#[derive(Debug, thiserror::Error)]
#[error("Invalid address: {0}")]
pub struct AddressValidationError(pub String);

mod assets;
mod foreign_assets;
mod pool_assets;

pub use assets::{query_all_assets_id, query_assets};
pub use foreign_assets::{
    parse_foreign_asset_locations, query_all_foreign_asset_locations, query_foreign_assets,
};
pub use pool_assets::{query_all_pool_assets_id, query_pool_assets};

// ================================================================================================
// Address Validation
// ================================================================================================

/// Validate and parse account address (supports SS58 and hex formats)
pub fn validate_and_parse_address(addr: &str) -> Result<AccountId32, AddressValidationError> {
    // Try SS58 format first (any network prefix)
    if let Ok(account) = AccountId32::from_str(addr) {
        return Ok(account);
    }

    // Try hex format (0x-prefixed, 32 bytes)
    if addr.starts_with("0x") && addr.len() == 66 {
        // 0x + 64 hex chars = 32 bytes
        if let Ok(bytes) = hex::decode(&addr[2..])
            && bytes.len() == 32
        {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            return Ok(AccountId32::from(arr));
        }
    }

    Err(AddressValidationError(addr.to_string()))
}

// ================================================================================================
// SS58 Network Name Lookup
// ================================================================================================

/// Get the network name for a given SS58 prefix.
/// Uses the official ss58-registry crate from https://github.com/paritytech/ss58-registry
///
/// For unknown prefixes, returns a generic name like "unknown-{prefix}"
/// to support custom networks and provide consistent API responses.
pub fn get_network_name(prefix: u16) -> Option<String> {
    use sp_core::crypto::Ss58AddressFormat;
    use ss58_registry::Ss58AddressFormatRegistry;

    let format = Ss58AddressFormat::custom(prefix);
    match Ss58AddressFormatRegistry::try_from(format) {
        Ok(registry) => Some(registry.to_string()),
        Err(_) => {
            // For unknown prefixes, return a generic name
            // This is more flexible for custom networks
            Some(format!("unknown-{}", prefix))
        }
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
