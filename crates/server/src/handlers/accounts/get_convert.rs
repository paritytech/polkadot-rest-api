use super::types::{AccountConvertQueryParams, AccountConvertResponse, AccountsError};
use super::utils::get_network_name;
use axum::{
    Json,
    extract::{Path, Query},
    response::{IntoResponse, Response},
};
use sp_core::crypto::{AccountId32, Ss58AddressFormat, Ss58Codec};

// ================================================================================================
// Main Handler
// ================================================================================================

/// Handler for GET /accounts/{accountId}/convert
///
/// Converts an AccountId or Public Key (hex) to an SS58 address.
///
/// Path Parameters:
/// - `accountId`: The AccountId or Public Key as hex string (with or without 0x prefix)
///
/// Query Parameters:
/// - `scheme` (optional): Cryptographic scheme - "ed25519", "sr25519", or "ecdsa" (default: "sr25519")
/// - `prefix` (optional): SS58 prefix number (default: 42)
/// - `publicKey` (optional): If true, treat the input as a public key (default: false)
#[utoipa::path(
    get,
    path = "/v1/accounts/{accountId}/convert",
    tag = "accounts",
    summary = "Convert account format",
    description = "Converts an account address between different SS58 formats and key types.",
    params(
        ("accountId" = String, Path, description = "SS58-encoded account address or hex public key"),
        ("scheme" = Option<String>, Query, description = "Cryptographic scheme: ed25519, sr25519, or ecdsa (default: sr25519)"),
        ("prefix" = Option<u16>, Query, description = "SS58 prefix number (default: 42)"),
        ("publicKey" = Option<bool>, Query, description = "If true, treat input as a public key")
    ),
    responses(
        (status = 200, description = "Converted account information", body = Object),
        (status = 400, description = "Invalid parameters"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_convert(
    Path(account_id): Path<String>,
    Query(params): Query<AccountConvertQueryParams>,
) -> Result<Response, AccountsError> {
    // Get scheme with default, validate
    let scheme = params
        .scheme
        .unwrap_or_else(|| "sr25519".to_string())
        .to_lowercase();
    if scheme != "ed25519" && scheme != "sr25519" && scheme != "ecdsa" {
        return Err(AccountsError::InvalidScheme);
    }

    // Get prefix with default
    let prefix = params.prefix.unwrap_or(42);

    // Validate that account_id is valid hex
    let account_id_clean = account_id.trim_start_matches("0x");
    if !is_valid_hex(account_id_clean) {
        return Err(AccountsError::InvalidHexAccountId);
    }

    // Get the network name for this prefix
    let network = get_network_name(prefix).ok_or(AccountsError::InvalidPrefix)?;

    // Decode the hex to bytes
    let account_bytes =
        hex::decode(account_id_clean).map_err(|_| AccountsError::InvalidHexAccountId)?;

    // For ecdsa with public key > 32 bytes, we need to hash it first
    let final_bytes = if params.public_key && scheme == "ecdsa" && account_bytes.len() > 32 {
        // Hash with blake2_256
        sp_core::blake2_256(&account_bytes).to_vec()
    } else {
        account_bytes
    };

    // Convert to AccountId32 (requires exactly 32 bytes)
    if final_bytes.len() != 32 {
        return Err(AccountsError::EncodingFailed(format!(
            "Expected 32 bytes, got {}",
            final_bytes.len()
        )));
    }

    let mut account_id_bytes = [0u8; 32];
    account_id_bytes.copy_from_slice(&final_bytes);

    let account_id32 = AccountId32::new(account_id_bytes);

    // Encode to SS58
    let ss58_format = Ss58AddressFormat::custom(prefix);
    let address = account_id32.to_ss58check_with_version(ss58_format);

    let response = AccountConvertResponse {
        ss58_prefix: prefix,
        network,
        address,
        account_id: format!("0x{}", account_id_clean),
        scheme: scheme.to_string(),
        public_key: params.public_key,
    };

    Ok(Json(response).into_response())
}

// ================================================================================================
// Helper Functions
// ================================================================================================

/// Check if a string is valid hexadecimal
fn is_valid_hex(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_hexdigit())
}
