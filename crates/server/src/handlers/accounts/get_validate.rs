// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use super::types::{AccountValidateQueryParams, AccountsError, AddressDetails};
use super::utils::validate_address;
use crate::extractors::JsonQuery;
use axum::{
    Json,
    extract::Path,
    response::{IntoResponse, Response},
};

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
        ("accountId" = String, Path, description = "SS58-encoded account address to validate"),
        ("at" = Option<String>, Query, description = "Block hash or number (accepted for API consistency)")
    ),
    responses(
        (status = 200, description = "Validation result", body = AddressDetails)
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
