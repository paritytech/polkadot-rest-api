// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Utility functions for account-related handlers.

use scale_value::{Value, ValueDef};
mod address;
mod assets;
mod foreign_assets;
mod pool_assets;

pub use address::{
    AddressValidationError, get_network_name, validate_address, validate_and_parse_address,
};
pub use assets::{query_all_assets_id, query_assets};
pub use foreign_assets::{
    parse_foreign_asset_locations, query_all_foreign_asset_locations, query_foreign_assets,
};
pub use pool_assets::{query_all_pool_assets_id, query_pool_assets};

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
