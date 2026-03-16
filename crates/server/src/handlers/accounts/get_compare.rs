// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use super::types::{
    AccountCompareQueryParams, AccountCompareResponse, AccountsError, AddressDetails,
};
use super::utils::validate_address;
use crate::extractors::JsonQuery;
use axum::{
    Json,
    response::{IntoResponse, Response},
};

// ================================================================================================
// Main Handler
// ================================================================================================

/// Handler for GET /accounts/compare
///
/// Compares up to 30 SS58 addresses and returns if they are equal or not,
/// along with details of each address. Equality is determined by comparing
/// the accountId/publicKey of each address.
///
/// Query Parameters:
/// - `addresses`: Comma-separated list of SS58 addresses to compare (max 30)
///
/// Returns:
/// - `areEqual`: Whether all addresses have the same underlying public key
/// - `addresses`: Array of address details with ss58Format, ss58Prefix, network, publicKey
#[utoipa::path(
    get,
    path = "/v1/accounts/compare",
    tag = "accounts",
    summary = "Compare account addresses",
    description = "Compares multiple SS58 addresses to determine if they have the same underlying public key.",
    params(
        ("addresses" = String, Query, description = "Comma-separated list of SS58 addresses to compare (max 30)")
    ),
    responses(
        (status = 200, description = "Comparison result", body = AccountCompareResponse),
        (status = 400, description = "Invalid parameters")
    )
)]
pub async fn get_compare(
    JsonQuery(params): JsonQuery<AccountCompareQueryParams>,
) -> Result<Response, AccountsError> {
    // Parse comma-separated addresses
    let addresses: Vec<&str> = params
        .addresses
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    // Validate address count
    if addresses.is_empty() {
        return Err(AccountsError::NoAddresses);
    }
    if addresses.len() > 30 {
        return Err(AccountsError::TooManyAddresses);
    }

    // Validate each address and collect details
    let address_details: Vec<AddressDetails> = addresses
        .iter()
        .map(|addr| validate_address(addr))
        .collect();

    // Check if all addresses have the same public key (only for valid addresses)
    let are_equal = {
        let first_public_key = address_details.first().and_then(|d| d.public_key.as_ref());
        match first_public_key {
            Some(first_key) => address_details
                .iter()
                .all(|d| d.public_key.as_ref() == Some(first_key)),
            None => false, // First address is invalid, so not equal
        }
    };

    let response = AccountCompareResponse {
        are_equal,
        addresses: address_details,
    };

    Ok(Json(response).into_response())
}
