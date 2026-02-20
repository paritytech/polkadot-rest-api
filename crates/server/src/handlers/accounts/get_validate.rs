// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use super::types::{AccountValidateQueryParams, AccountValidateResponse, AccountsError};
use super::utils::get_network_name;
use crate::extractors::JsonQuery;
use axum::{
    Json,
    extract::Path,
    response::{IntoResponse, Response},
};
use sp_core::crypto::{AccountId32, Ss58Codec};

// ================================================================================================
// Main Handler
// ================================================================================================

/// Handler for GET /accounts/{accountId}/validate
///
/// Validates an SS58 or hex-encoded address and returns information about it.
///
/// Path Parameters:
/// - `accountId`: The address to validate (SS58 format or hex-encoded SS58)
///
/// Returns:
/// - `isValid`: Whether the address is valid
/// - `ss58Prefix`: The SS58 prefix (null if invalid)
/// - `network`: The network name for the prefix (null if invalid/unknown)
/// - `accountId`: The account ID in hex format (null if invalid)
#[utoipa::path(
    get,
    path = "/v1/accounts/{accountId}/validate",
    tag = "accounts",
    summary = "Validate account address",
    description = "Validates an SS58-encoded account address and returns details about its format.",
    params(
        ("accountId" = String, Path, description = "SS58-encoded account address to validate")
    ),
    responses(
        (status = 200, description = "Validation result", body = AccountValidateResponse)
    )
)]
pub async fn get_validate(
    Path(address): Path<String>,
    JsonQuery(_params): JsonQuery<AccountValidateQueryParams>,
) -> Result<Response, AccountsError> {
    // Note: `at` param is accepted for API consistency but not used for validation
    let result = validate_address(&address);
    Ok(Json(result).into_response())
}

// ================================================================================================
// Validation Logic
// ================================================================================================

fn validate_address(address: &str) -> AccountValidateResponse {
    // Check if the address is hex format (0x prefix or all hex chars)
    let is_hex = address.starts_with("0x");

    if is_hex {
        validate_hex_address(address)
    } else {
        validate_ss58_address(address)
    }
}

/// Validate a hex-encoded SS58 address
/// The hex should decode to valid SS58 bytes: prefix byte(s) + account id (32 bytes) + checksum (2 bytes)
fn validate_hex_address(address: &str) -> AccountValidateResponse {
    let hex_str = address.trim_start_matches("0x");

    // Decode hex to bytes
    let bytes = match hex::decode(hex_str) {
        Ok(b) => b,
        Err(_) => return invalid_response(),
    };

    // Valid SS58 encoded lengths
    // 35 = 1 byte prefix + 32 bytes account + 2 bytes checksum
    // 36 = 2 byte prefix + 32 bytes account + 2 bytes checksum
    if bytes.len() != 35 && bytes.len() != 36 {
        return invalid_response();
    }

    // Extract the prefix
    let (prefix, account_start) = if bytes[0] < 64 {
        // Single byte prefix (0-63)
        (bytes[0] as u16, 1)
    } else if bytes[0] < 128 && bytes.len() == 36 {
        // Two byte prefix (64-16383)
        // Decode: lower 6 bits of byte[0] combined with byte[1]
        let lower = (bytes[0] & 0x3f) as u16;
        let upper = bytes[1] as u16;
        let prefix = lower | (upper << 6);
        (prefix, 2)
    } else {
        return invalid_response();
    };

    // Extract the account ID (32 bytes after prefix)
    if bytes.len() < account_start + 32 + 2 {
        return invalid_response();
    }

    let account_bytes = &bytes[account_start..account_start + 32];

    // Verify checksum using sp_core's SS58 implementation
    // We reconstruct the SS58 address and validate it
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
                return invalid_response();
            }

            // Verify the prefix matches
            if decoded_prefix != prefix {
                return invalid_response();
            }

            // Now verify the checksum in the original bytes
            // Re-encode and compare the hex
            let re_encoded = encode_ss58_to_bytes(&account_id, prefix);
            if re_encoded != bytes {
                return invalid_response();
            }

            let network = get_network_name(prefix);
            AccountValidateResponse {
                is_valid: true,
                ss58_prefix: Some(prefix.to_string()),
                network,
                account_id: Some(format!("0x{}", hex::encode(account_bytes))),
            }
        }
        Err(_) => invalid_response(),
    }
}

/// Validate an SS58-encoded address
fn validate_ss58_address(address: &str) -> AccountValidateResponse {
    // Try to decode the SS58 address using sp_core
    match AccountId32::from_ss58check_with_version(address) {
        Ok((account_id, ss58_format)) => {
            let prefix = u16::from(ss58_format);
            let network = get_network_name(prefix);
            let account_bytes: &[u8; 32] = account_id.as_ref();
            AccountValidateResponse {
                is_valid: true,
                ss58_prefix: Some(prefix.to_string()),
                network,
                account_id: Some(format!("0x{}", hex::encode(account_bytes))),
            }
        }
        Err(_) => invalid_response(),
    }
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

fn invalid_response() -> AccountValidateResponse {
    AccountValidateResponse {
        is_valid: false,
        ss58_prefix: None,
        network: None,
        account_id: None,
    }
}
