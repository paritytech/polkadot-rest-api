use crate::handlers::accounts::AddressDetails;
use sp_core::crypto::{AccountId32, Ss58Codec};

/// Error type for address validation failures.
/// This is a standalone error type that can be converted to handler-specific errors via #[from].
#[derive(Debug, thiserror::Error)]
#[error("Invalid address: {0}")]
pub struct AddressValidationError(pub String);

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

/// Validate and parse account address (supports SS58 and hex formats)
///
/// For SS58 addresses, validates that the address uses the expected network prefix.
/// Hex addresses (0x-prefixed) are accepted regardless of prefix.
pub fn validate_and_parse_address(
    addr: &str,
    ss58_prefix: u16,
) -> Result<AccountId32, AddressValidationError> {
    use sp_core::crypto::Ss58AddressFormat;

    // Try SS58 format first - decode and validate the prefix matches
    if let Ok((account, version)) = AccountId32::from_ss58check_with_version(addr) {
        let expected_format = Ss58AddressFormat::custom(ss58_prefix);
        if version == expected_format {
            return Ok(account);
        }
        // Address decoded but wrong network prefix
        return Err(AddressValidationError(format!(
            "Address '{}' uses SS58 prefix {} but expected prefix {}",
            addr,
            u16::from(version),
            ss58_prefix
        )));
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

/// Calculate SS58 checksum (first 2 bytes of blake2b hash with SS58PRE prefix)
fn ss58_checksum(data: &[u8]) -> [u8; 2] {
    use sp_core::hashing::blake2_512;

    const SS58_PREFIX: &[u8] = b"SS58PRE";

    let mut input = Vec::with_capacity(SS58_PREFIX.len() + data.len());
    input.extend_from_slice(SS58_PREFIX);
    input.extend_from_slice(data);

    let hash = blake2_512(&input);
    [hash[0], hash[1]]
}

/// Encode an account ID to SS58 bytes (prefix + account + checksum)
fn encode_ss58_to_bytes(account: &AccountId32, prefix: u16) -> Vec<u8> {
    let mut result = Vec::new();

    // Encode prefix
    if prefix < 64 {
        result.push(prefix as u8);
    } else {
        // Two-byte prefix encoding
        let first = 0x40 | ((prefix & 0x3f) as u8);
        let second = ((prefix >> 6) & 0xff) as u8;
        result.push(first);
        result.push(second);
    }

    // Add account ID
    result.extend_from_slice(account.as_ref());

    // Calculate and add checksum
    let checksum = ss58_checksum(&result);
    result.push(checksum[0]);
    result.push(checksum[1]);

    result
}

fn invalid_address_details(address: &str) -> AddressDetails {
    AddressDetails {
        ss58_format: address.to_string(),
        ss58_prefix: None,
        network: None,
        public_key: None,
    }
}

/// Validate an SS58-encoded address
fn validate_ss58_address(address: &str) -> AddressDetails {
    // Try to decode the SS58 address using sp_core
    match AccountId32::from_ss58check_with_version(address) {
        Ok((account_id, ss58_format)) => {
            let prefix = u16::from(ss58_format);
            let network = get_network_name(prefix);
            let account_bytes: &[u8; 32] = account_id.as_ref();
            AddressDetails {
                ss58_format: address.to_string(),
                ss58_prefix: Some(prefix),
                network,
                public_key: Some(format!("0x{}", hex::encode(account_bytes))),
            }
        }
        Err(e) => {
            tracing::debug!("Failed to validate address: {:?}", e);
            invalid_address_details(address)
        }
    }
}

/// Validate a hex-encoded SS58 address
fn validate_hex_address(address: &str) -> AddressDetails {
    let hex_str = address.trim_start_matches("0x");

    // Decode hex to bytes
    let bytes = match hex::decode(hex_str) {
        Ok(b) => b,
        Err(e) => {
            tracing::debug!("Failed to decode hex address: {:?}", e);
            return invalid_address_details(address);
        }
    };

    // Valid SS58 encoded lengths
    // 35 = 1 byte prefix + 32 bytes account + 2 bytes checksum
    // 36 = 2 byte prefix + 32 bytes account + 2 bytes checksum
    if bytes.len() != 35 && bytes.len() != 36 {
        return invalid_address_details(address);
    }

    // Extract the prefix
    let (prefix, account_start) = if bytes[0] < 64 {
        // Single byte prefix (0-63)
        (bytes[0] as u16, 1)
    } else if bytes[0] < 128 && bytes.len() == 36 {
        // Two byte prefix (64-16383)
        let lower = (bytes[0] & 0x3f) as u16;
        let upper = bytes[1] as u16;
        let prefix = lower | (upper << 6);
        (prefix, 2)
    } else {
        return invalid_address_details(address);
    };

    // Extract the account ID (32 bytes after prefix)
    if bytes.len() < account_start + 32 + 2 {
        return invalid_address_details(address);
    }

    let account_bytes = &bytes[account_start..account_start + 32];

    // Verify checksum using sp_core's SS58 implementation
    let mut account_arr = [0u8; 32];
    account_arr.copy_from_slice(account_bytes);
    let account_id = AccountId32::new(account_arr);

    // Encode back to SS58 and decode to verify checksum
    let ss58_format = sp_core::crypto::Ss58AddressFormat::custom(prefix);
    let ss58_address = account_id.to_ss58check_with_version(ss58_format);

    // Now decode it back to verify the original bytes match
    match AccountId32::from_ss58check_with_version(&ss58_address) {
        Ok((decoded_account, decoded_format)) => {
            let decoded_prefix = u16::from(decoded_format);

            // Verify the account ID matches
            let decoded_bytes: &[u8; 32] = decoded_account.as_ref();
            if decoded_bytes != account_bytes {
                return invalid_address_details(address);
            }

            // Verify the prefix matches
            if decoded_prefix != prefix {
                return invalid_address_details(address);
            }

            // Now verify the checksum in the original bytes
            let re_encoded = encode_ss58_to_bytes(&account_id, prefix);
            if re_encoded != bytes {
                return invalid_address_details(address);
            }

            let network = get_network_name(prefix);
            AddressDetails {
                ss58_format: address.to_string(),
                ss58_prefix: Some(prefix),
                network,
                public_key: Some(format!("0x{}", hex::encode(account_bytes))),
            }
        }
        Err(e) => {
            tracing::debug!("Failed to decode SS58 address: {:?}", e);
            invalid_address_details(address)
        }
    }
}

pub fn validate_address(address: &str) -> AddressDetails {
    // Check if the address is hex format (0x prefix)
    let is_hex = address.starts_with("0x");

    if is_hex {
        validate_hex_address(address)
    } else {
        validate_ss58_address(address)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Common SS58 prefixes for testing
    const POLKADOT_PREFIX: u16 = 0;
    const KUSAMA_PREFIX: u16 = 2;
    const SUBSTRATE_PREFIX: u16 = 42;

    #[test]
    fn test_address_validation_hex() {
        // Alice's address in hex - should work with any prefix
        let addr = "0xd43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d";
        assert!(validate_and_parse_address(addr, POLKADOT_PREFIX).is_ok());
        assert!(validate_and_parse_address(addr, KUSAMA_PREFIX).is_ok());
    }

    #[test]
    fn test_address_validation_ss58_polkadot() {
        // Alice's address in SS58 (Polkadot prefix 0)
        let addr = "15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5";
        // Should succeed with correct prefix
        assert!(validate_and_parse_address(addr, POLKADOT_PREFIX).is_ok());
        // Should fail with wrong prefix
        assert!(validate_and_parse_address(addr, KUSAMA_PREFIX).is_err());
    }

    #[test]
    fn test_address_validation_ss58_substrate() {
        // Alice's address in SS58 (Substrate generic prefix 42)
        let addr = "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY";
        // Should succeed with correct prefix
        assert!(validate_and_parse_address(addr, SUBSTRATE_PREFIX).is_ok());
        // Should fail with wrong prefix
        assert!(validate_and_parse_address(addr, POLKADOT_PREFIX).is_err());
    }

    #[test]
    fn test_address_validation_invalid() {
        let addr = "invalid-address";
        assert!(validate_and_parse_address(addr, POLKADOT_PREFIX).is_err());
    }

    #[test]
    fn test_address_validation_short_hex() {
        let addr = "0x1234"; // Too short
        assert!(validate_and_parse_address(addr, POLKADOT_PREFIX).is_err());
    }

    #[test]
    fn test_address_validation_wrong_prefix_error_message() {
        // Polkadot address with Kusama prefix should give informative error
        let addr = "15oF4uVJwmo4TdGW7VfQxNLavjCXviqxT9S1MgbjMNHr6Sp5";
        let result = validate_and_parse_address(addr, KUSAMA_PREFIX);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.0.contains("prefix"));
    }
}
