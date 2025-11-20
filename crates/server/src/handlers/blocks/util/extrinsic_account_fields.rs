//! Extrinsic AccountId32 field name patterns
//!
//! This module provides field name pattern matching for identifying AccountId32 fields
//! in extrinsic call arguments. Unlike events (which require position mappings), extrinsics
//! expose field names through subxt-historic, allowing for a generic name-based approach.
//!
//! ## Why Name-Based?
//!
//! When iterating through extrinsic call fields via `extrinsic.call().fields().iter()`,
//! we can access each field's name using `.name()`. This allows us to check if the field
//! name matches common AccountId32 patterns like "dest", "who", "target", etc.
//!
//! ## Generic Approach
//!
//! This approach works across all pallets and extrinsics without hardcoding specific
//! call types. Any extrinsic with a field named "dest" will have that field converted
//! to SS58 format, regardless of which pallet it's from.
//!
//! ## Maintenance
//!
//! When adding new field name patterns:
//! 1. Verify the pattern is commonly used for AccountId32 across multiple pallets
//! 2. Avoid overly generic names that might match non-account fields
//! 3. Add the pattern to the match statement in `is_account_field()`
//!
//! ## Example
//!
//! For `balances.transfer_keep_alive(dest: MultiAddress, value: Balance)`:
//! - Field "dest" matches the pattern ✓ → converted to SS58
//! - Field "value" doesn't match ✗ → stays as-is

/// Checks if a field name indicates it contains an AccountId32 value
///
/// This function uses pattern matching on common field names to identify which
/// extrinsic call arguments should be converted to SS58 addresses. This approach
/// is generic and works across all pallets without hardcoding specific calls.
///
/// # Arguments
///
/// * `field_name` - The field name from the extrinsic call (e.g., "dest", "who")
///
/// # Returns
///
/// `true` if the field name matches a known AccountId32 pattern, `false` otherwise
///
/// # Examples
///
/// ```
/// # use server::handlers::blocks::util::extrinsic_account_fields::is_account_field;
/// assert!(is_account_field("dest"));
/// assert!(is_account_field("who"));
/// assert!(is_account_field("target"));
/// assert!(!is_account_field("value"));
/// assert!(!is_account_field("amount"));
/// ```
pub fn is_account_field(field_name: &str) -> bool {
    matches!(
        field_name,
        "dest"
            | "source"
            | "target"
            | "targets"
            | "beneficiary"
            | "controller"
            | "stash"
            | "payee"
            | "account"
            | "who"
            | "real"
            | "delegate"
            | "delegator"
            | "delegatee"
            | "nominee"
            | "nominator"
            | "validator"
            | "validators"
            | "new_"
            | "old"
            | "sender"
            | "receiver"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_common_account_fields() {
        // Most common patterns
        assert!(is_account_field("dest"));
        assert!(is_account_field("who"));
        assert!(is_account_field("target"));
        assert!(is_account_field("account"));
    }

    #[test]
    fn test_staking_fields() {
        assert!(is_account_field("controller"));
        assert!(is_account_field("stash"));
        assert!(is_account_field("nominator"));
        assert!(is_account_field("validator"));
        assert!(is_account_field("validators"));
    }

    #[test]
    fn test_proxy_multisig_fields() {
        assert!(is_account_field("delegate"));
        assert!(is_account_field("delegator"));
        assert!(is_account_field("delegatee"));
        assert!(is_account_field("real"));
    }

    #[test]
    fn test_non_account_fields() {
        // These should NOT match
        assert!(!is_account_field("value"));
        assert!(!is_account_field("amount"));
        assert!(!is_account_field("balance"));
        assert!(!is_account_field("call"));
        assert!(!is_account_field("data"));
        assert!(!is_account_field("hash"));
        assert!(!is_account_field("proof"));
    }

    #[test]
    fn test_vec_fields() {
        assert!(is_account_field("targets"));
        assert!(is_account_field("validators"));
    }

    #[test]
    fn test_sender_receiver() {
        assert!(is_account_field("sender"));
        assert!(is_account_field("receiver"));
        assert!(is_account_field("source"));
    }
}
