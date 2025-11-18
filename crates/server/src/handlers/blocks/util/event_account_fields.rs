//! Event AccountId32 field position mappings
//!
//! This module provides mappings between Substrate runtime events and the positions
//! of their AccountId32 fields. These mappings are used to convert raw account bytes
//! to SS58 addresses in the API response.
//!
//! ## Why Position-Based?
//!
//! Unlike extrinsics (which expose field names via subxt-historic), events are decoded
//! as anonymous arrays without field name information. Therefore, we maintain this
//! explicit mapping of (pallet, event) -> field positions.
//!
//! ## Maintenance
//!
//! When adding support for new pallets or events:
//! 1. Check the runtime metadata or source code for the event definition
//! 2. Identify which fields are AccountId32 types
//! 3. Add a mapping entry with the zero-indexed positions
//!
//! ## Example
//!
//! For `balances.Transfer(from: AccountId32, to: AccountId32, amount: Balance)`:
//! - Position 0: `from` (AccountId32) ✓
//! - Position 1: `to` (AccountId32) ✓
//! - Position 2: `amount` (Balance) ✗
//!
//! Mapping: `("balances", "Transfer") => vec![0, 1]`

/// Returns a set of field positions that contain AccountId32 values for a given event type
///
/// This mapping is used to selectively convert event field bytes to SS58 addresses,
/// avoiding false positives with other 32-byte values like H256 hashes.
///
/// # Arguments
///
/// * `pallet` - The lowercase pallet name (e.g., "balances")
/// * `event` - The event name (e.g., "Transfer")
///
/// # Returns
///
/// A vector of zero-indexed field positions that contain AccountId32 values.
/// Returns an empty vector if the event has no account fields or is not mapped.
pub fn get_account_field_positions(pallet: &str, event: &str) -> Vec<usize> {
    match (pallet, event) {
        // Balances pallet events
        ("balances", "Deposit") => vec![0],               // who
        ("balances", "Transfer") => vec![0, 1],           // from, to
        ("balances", "Withdraw") => vec![0],              // who
        ("balances", "Reserved") => vec![0],              // who
        ("balances", "Unreserved") => vec![0],            // who
        ("balances", "ReserveRepatriated") => vec![0, 1], // from, to
        ("balances", "BalanceSet") => vec![0],            // who
        ("balances", "Endowed") => vec![0],               // account
        ("balances", "DustLost") => vec![0],              // account
        ("balances", "Slashed") => vec![0],               // who
        ("balances", "Minted") => vec![0],                // who
        ("balances", "Burned") => vec![0],                // who
        ("balances", "Suspended") => vec![0],             // who
        ("balances", "Restored") => vec![0],              // who
        ("balances", "Upgraded") => vec![0],              // who
        ("balances", "Issued") => vec![],                 // amount only
        ("balances", "Rescinded") => vec![],              // amount only
        ("balances", "Locked") => vec![0],                // who
        ("balances", "Unlocked") => vec![0],              // who
        ("balances", "Frozen") => vec![0],                // who
        ("balances", "Thawed") => vec![0],                // who

        // Staking pallet events
        ("staking", "Bonded") => vec![0],        // stash
        ("staking", "Unbonded") => vec![0],      // stash
        ("staking", "Withdrawn") => vec![0],     // stash
        ("staking", "Rewarded") => vec![0],      // stash
        ("staking", "Slashed") => vec![0],       // validator/nominator
        ("staking", "SlashReported") => vec![0], // validator
        ("staking", "OldSlashingReportDiscarded") => vec![], // session_index only
        ("staking", "StakersElected") => vec![], // no accounts
        ("staking", "ForceEra") => vec![],       // mode only
        ("staking", "ValidatorPrefsSet") => vec![0], // stash
        ("staking", "SnapshotVotersSizeExceeded") => vec![], // size only
        ("staking", "SnapshotTargetsSizeExceeded") => vec![], // size only
        ("staking", "Chilled") => vec![0],       // stash
        ("staking", "PayoutStarted") => vec![1], // validator (era_index, validator_stash)
        ("staking", "Kicked") => vec![0, 1],     // nominator, stash

        // Session pallet events
        ("session", "NewSession") => vec![], // session_index only

        // Treasury pallet events
        ("treasury", "Proposed") => vec![], // proposal_index only
        ("treasury", "Spending") => vec![], // budget_remaining only
        ("treasury", "Awarded") => vec![1], // beneficiary (proposal_index, award, account)
        ("treasury", "Rejected") => vec![], // proposal_index, slashed only
        ("treasury", "Burnt") => vec![],    // burnt_funds only
        ("treasury", "Rollover") => vec![], // rollover_balance only
        ("treasury", "Deposit") => vec![],  // value only
        ("treasury", "SpendApproved") => vec![2], // beneficiary (proposal_index, amount, beneficiary)

        // Identity pallet events
        ("identity", "IdentitySet") => vec![0],        // who
        ("identity", "IdentityCleared") => vec![0],    // who
        ("identity", "IdentityKilled") => vec![0],     // who
        ("identity", "JudgementRequested") => vec![0], // who
        ("identity", "JudgementUnrequested") => vec![0], // who
        ("identity", "JudgementGiven") => vec![0],     // target
        ("identity", "RegistrarAdded") => vec![],      // registrar_index only
        ("identity", "SubIdentityAdded") => vec![0, 1], // sub, main
        ("identity", "SubIdentityRemoved") => vec![0, 1], // sub, main
        ("identity", "SubIdentityRevoked") => vec![0, 1], // sub, main

        // Proxy pallet events
        ("proxy", "ProxyExecuted") => vec![], // result only (no direct account in event data)
        ("proxy", "PureCreated") => vec![0, 1], // pure, who
        ("proxy", "Announced") => vec![0, 1], // real, proxy
        ("proxy", "ProxyAdded") => vec![0, 1], // delegator, delegatee
        ("proxy", "ProxyRemoved") => vec![0, 1], // delegator, delegatee

        // Multisig pallet events
        ("multisig", "NewMultisig") => vec![0, 1], // approving, multisig
        ("multisig", "MultisigApproval") => vec![0, 1], // approving, multisig
        ("multisig", "MultisigExecuted") => vec![0, 1], // approving, multisig
        ("multisig", "MultisigCancelled") => vec![0, 1], // cancelling, multisig

        // Vesting pallet events
        ("vesting", "VestingUpdated") => vec![0], // account
        ("vesting", "VestingCompleted") => vec![0], // account

        // Utility pallet events
        ("utility", "BatchInterrupted") => vec![], // index, error only
        ("utility", "BatchCompleted") => vec![],   // no accounts
        ("utility", "BatchCompletedWithErrors") => vec![], // no accounts
        ("utility", "ItemCompleted") => vec![],    // no accounts
        ("utility", "ItemFailed") => vec![],       // error only
        ("utility", "DispatchedAs") => vec![],     // result only

        // Transaction Payment pallet events
        ("transactionpayment", "TransactionFeePaid") => vec![0], // who

        // Default: no account fields
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_balances_transfer() {
        assert_eq!(
            get_account_field_positions("balances", "Transfer"),
            vec![0, 1]
        );
    }

    #[test]
    fn test_balances_deposit() {
        assert_eq!(get_account_field_positions("balances", "Deposit"), vec![0]);
    }

    #[test]
    fn test_unknown_event() {
        assert_eq!(
            get_account_field_positions("unknown", "Event"),
            Vec::<usize>::new()
        );
    }

    #[test]
    fn test_transactionpayment_fee_paid() {
        assert_eq!(
            get_account_field_positions("transactionpayment", "TransactionFeePaid"),
            vec![0]
        );
    }
}
