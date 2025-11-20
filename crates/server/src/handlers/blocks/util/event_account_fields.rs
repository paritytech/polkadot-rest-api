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

        // Democracy pallet events
        ("democracy", "Proposed") => vec![], // proposal_index, deposit only
        ("democracy", "Tabled") => vec![],   // proposal_index, deposit only
        ("democracy", "ExternalTabled") => vec![], // no accounts
        ("democracy", "Started") => vec![],  // ref_index, threshold only
        ("democracy", "Passed") => vec![],   // ref_index only
        ("democracy", "NotPassed") => vec![], // ref_index only
        ("democracy", "Cancelled") => vec![], // ref_index only
        ("democracy", "Delegated") => vec![0, 1], // who, target
        ("democracy", "Undelegated") => vec![0], // account
        ("democracy", "Vetoed") => vec![0],  // who (ref_index, who, until)
        ("democracy", "Blacklisted") => vec![], // proposal_hash only
        ("democracy", "Voted") => vec![0],   // voter (voter, ref_index, vote)
        ("democracy", "Seconded") => vec![0], // seconder (seconder, prop_index)
        ("democracy", "ProposalCanceled") => vec![], // prop_index only
        ("democracy", "MetadataSet") => vec![], // owner, hash only
        ("democracy", "MetadataCleared") => vec![], // owner, hash only
        ("democracy", "MetadataTransferred") => vec![0, 1], // prev_owner, owner

        // Council pallet events
        ("council", "Proposed") => vec![0], // account (account, proposal_index, proposal_hash, threshold)
        ("council", "Voted") => vec![0],    // account (account, proposal_hash, voted, yes, no)
        ("council", "Approved") => vec![],  // proposal_hash only
        ("council", "Disapproved") => vec![], // proposal_hash only
        ("council", "Executed") => vec![],  // proposal_hash, result only
        ("council", "MemberExecuted") => vec![], // proposal_hash, result only
        ("council", "Closed") => vec![],    // proposal_hash, yes, no only

        // TechnicalCommittee pallet events
        ("technicalcommittee", "Proposed") => vec![0], // account
        ("technicalcommittee", "Voted") => vec![0],    // account
        ("technicalcommittee", "Approved") => vec![],  // proposal_hash only
        ("technicalcommittee", "Disapproved") => vec![], // proposal_hash only
        ("technicalcommittee", "Executed") => vec![],  // proposal_hash, result only
        ("technicalcommittee", "MemberExecuted") => vec![], // proposal_hash, result only
        ("technicalcommittee", "Closed") => vec![],    // proposal_hash, yes, no only

        // Elections (Phragmen/PhragmenElection) pallet events
        ("phragmenelection", "NewTerm") => vec![], // members only (Vec<(AccountId, Balance)>)
        ("phragmenelection", "EmptyTerm") => vec![], // no accounts
        ("phragmenelection", "ElectionError") => vec![], // no accounts
        ("phragmenelection", "MemberKicked") => vec![0], // member
        ("phragmenelection", "Renounced") => vec![0], // candidate
        ("phragmenelection", "CandidateSlashed") => vec![0], // candidate
        ("phragmenelection", "SeatHolderSlashed") => vec![0], // seat_holder

        // Referenda pallet events (OpenGov)
        ("referenda", "Submitted") => vec![], // index, track, proposal only
        ("referenda", "DecisionDepositPlaced") => vec![0], // who (index, who, amount)
        ("referenda", "DecisionDepositRefunded") => vec![0], // who
        ("referenda", "DepositSlashed") => vec![0], // who
        ("referenda", "DecisionStarted") => vec![], // index, track, proposal, tally only
        ("referenda", "ConfirmStarted") => vec![], // index only
        ("referenda", "ConfirmAborted") => vec![], // index only
        ("referenda", "Confirmed") => vec![], // index, tally only
        ("referenda", "Approved") => vec![],  // index only
        ("referenda", "Rejected") => vec![],  // index, tally only
        ("referenda", "TimedOut") => vec![],  // index, tally only
        ("referenda", "Cancelled") => vec![], // index, tally only
        ("referenda", "Killed") => vec![],    // index, tally only
        ("referenda", "SubmissionDepositRefunded") => vec![0], // who
        ("referenda", "MetadataSet") => vec![], // index, hash only
        ("referenda", "MetadataCleared") => vec![], // index, hash only

        // ConvictionVoting pallet events
        ("convictionvoting", "Delegated") => vec![0, 1], // who, target
        ("convictionvoting", "Undelegated") => vec![0],  // who

        // Whitelist pallet events
        ("whitelist", "CallWhitelisted") => vec![], // call_hash only
        ("whitelist", "WhitelistedCallRemoved") => vec![], // call_hash only
        ("whitelist", "WhitelistedCallDispatched") => vec![], // call_hash, result only

        // FellowshipCollective pallet events
        ("fellowshipcollective", "MemberAdded") => vec![0], // who
        ("fellowshipcollective", "RankChanged") => vec![0], // who
        ("fellowshipcollective", "MemberRemoved") => vec![0], // who
        ("fellowshipcollective", "Voted") => vec![0],       // who

        // FellowshipReferenda pallet events
        ("fellowshipreferenda", "Submitted") => vec![], // index, track, proposal only
        ("fellowshipreferenda", "DecisionDepositPlaced") => vec![0], // who
        ("fellowshipreferenda", "DecisionDepositRefunded") => vec![0], // who
        ("fellowshipreferenda", "DepositSlashed") => vec![0], // who
        ("fellowshipreferenda", "DecisionStarted") => vec![], // index, track, proposal, tally only
        ("fellowshipreferenda", "ConfirmStarted") => vec![], // index only
        ("fellowshipreferenda", "ConfirmAborted") => vec![], // index only
        ("fellowshipreferenda", "Confirmed") => vec![], // index, tally only
        ("fellowshipreferenda", "Approved") => vec![],  // index only
        ("fellowshipreferenda", "Rejected") => vec![],  // index, tally only
        ("fellowshipreferenda", "TimedOut") => vec![],  // index, tally only
        ("fellowshipreferenda", "Cancelled") => vec![], // index, tally only
        ("fellowshipreferenda", "Killed") => vec![],    // index, tally only
        ("fellowshipreferenda", "SubmissionDepositRefunded") => vec![0], // who

        // NominationPools pallet events
        ("nominationpools", "Created") => vec![0, 1], // depositor, pool_id -> (depositor, pool_id)
        ("nominationpools", "Bonded") => vec![0],     // member
        ("nominationpools", "PaidOut") => vec![0],    // member
        ("nominationpools", "Unbonded") => vec![0],   // member
        ("nominationpools", "Withdrawn") => vec![0],  // member
        ("nominationpools", "Destroyed") => vec![],   // pool_id only
        ("nominationpools", "StateChanged") => vec![], // pool_id, new_state only
        ("nominationpools", "MemberRemoved") => vec![0], // member (pool_id, member)
        ("nominationpools", "RolesUpdated") => vec![], // root, bouncer, nominator (may contain accounts but complex structure)
        ("nominationpools", "PoolSlashed") => vec![],  // pool_id, balance only
        ("nominationpools", "UnbondingPoolSlashed") => vec![], // pool_id, era, balance only
        ("nominationpools", "PoolCommissionUpdated") => vec![], // pool_id, current only
        ("nominationpools", "PoolMaxCommissionUpdated") => vec![], // pool_id, max_commission only
        ("nominationpools", "PoolCommissionChangeRateUpdated") => vec![], // pool_id, change_rate only
        ("nominationpools", "PoolCommissionClaimPermissionUpdated") => vec![], // pool_id, permission only

        // Assets pallet events
        ("assets", "Created") => vec![1], // creator (asset_id, creator, owner)
        ("assets", "Issued") => vec![1],  // owner (asset_id, owner, amount)
        ("assets", "Transferred") => vec![1, 2], // from, to (asset_id, from, to, amount)
        ("assets", "Burned") => vec![1],  // owner (asset_id, owner, balance)
        ("assets", "TeamChanged") => vec![], // asset_id, issuer, admin, freezer only
        ("assets", "OwnerChanged") => vec![1], // owner (asset_id, owner)
        ("assets", "Frozen") => vec![1],  // who (asset_id, who)
        ("assets", "Thawed") => vec![1],  // who (asset_id, who)
        ("assets", "AssetFrozen") => vec![], // asset_id only
        ("assets", "AssetThawed") => vec![], // asset_id only
        ("assets", "AccountsDestroyed") => vec![], // asset_id, accounts_destroyed, accounts_remaining only
        ("assets", "ApprovalsDestroyed") => vec![], // asset_id, approvals_destroyed, approvals_remaining only
        ("assets", "DestructionStarted") => vec![], // asset_id only
        ("assets", "Destroyed") => vec![],          // asset_id only
        ("assets", "ForceCreated") => vec![1],      // owner (asset_id, owner)
        ("assets", "MetadataSet") => vec![], // asset_id, name, symbol, decimals, is_frozen only
        ("assets", "MetadataCleared") => vec![], // asset_id only
        ("assets", "ApprovedTransfer") => vec![1, 2], // source, delegate (asset_id, source, delegate, amount)
        ("assets", "ApprovalCancelled") => vec![1, 2], // owner, delegate (asset_id, owner, delegate)
        ("assets", "TransferredApproved") => vec![1, 2, 3], // owner, delegate, destination
        ("assets", "AssetStatusChanged") => vec![],    // asset_id only
        ("assets", "AssetMinBalanceChanged") => vec![], // asset_id, new_min_balance only
        ("assets", "Touched") => vec![1],              // who (asset_id, who, depositor)
        ("assets", "Blocked") => vec![1],              // who (asset_id, who)

        // Uniques (NFT) pallet events
        ("uniques", "Created") => vec![1, 2], // creator, owner (collection, creator, owner)
        ("uniques", "ForceCreated") => vec![1], // owner (collection, owner)
        ("uniques", "Destroyed") => vec![],   // collection only
        ("uniques", "Issued") => vec![2],     // owner (collection, item, owner)
        ("uniques", "Transferred") => vec![2, 3], // from, to (collection, item, from, to)
        ("uniques", "Burned") => vec![2],     // owner (collection, item, owner)
        ("uniques", "Frozen") => vec![],      // collection, item only
        ("uniques", "Thawed") => vec![],      // collection, item only
        ("uniques", "CollectionFrozen") => vec![], // collection only
        ("uniques", "CollectionThawed") => vec![], // collection only
        ("uniques", "OwnerChanged") => vec![1], // new_owner (collection, new_owner)
        ("uniques", "TeamChanged") => vec![], // collection, issuer, admin, freezer only
        ("uniques", "ApprovedTransfer") => vec![2, 3], // owner, delegate (collection, item, owner, delegate)
        ("uniques", "ApprovalCancelled") => vec![2, 3], // owner, delegate
        ("uniques", "ItemStatusChanged") => vec![],    // collection only
        ("uniques", "CollectionMetadataSet") => vec![], // collection, data, is_frozen only
        ("uniques", "CollectionMetadataCleared") => vec![], // collection only
        ("uniques", "MetadataSet") => vec![],          // collection, item, data, is_frozen only
        ("uniques", "MetadataCleared") => vec![],      // collection, item only
        ("uniques", "Redeposited") => vec![],          // collection, successful_items only
        ("uniques", "AttributeSet") => vec![],         // collection, maybe_item, key, value only
        ("uniques", "AttributeCleared") => vec![],     // collection, maybe_item, key only
        ("uniques", "OwnershipAcceptanceChanged") => vec![0], // who
        ("uniques", "CollectionMaxSupplySet") => vec![], // collection, max_supply only
        ("uniques", "ItemPriceSet") => vec![], // collection, item, price, whitelisted_buyer only
        ("uniques", "ItemPriceRemoved") => vec![], // collection, item only
        ("uniques", "ItemBought") => vec![3, 4], // buyer, seller (collection, item, price, seller, buyer)

        // Nfts pallet events (newer NFT pallet)
        ("nfts", "Created") => vec![1, 2], // creator, owner (collection, creator, owner)
        ("nfts", "ForceCreated") => vec![1], // owner (collection, owner)
        ("nfts", "Destroyed") => vec![],   // collection only
        ("nfts", "Issued") => vec![2],     // owner (collection, item, owner)
        ("nfts", "Transferred") => vec![2, 3], // from, to (collection, item, from, to)
        ("nfts", "Burned") => vec![2],     // owner (collection, item, owner)
        ("nfts", "ItemTransferLocked") => vec![], // collection, item only
        ("nfts", "ItemTransferUnlocked") => vec![], // collection, item only
        ("nfts", "ItemPropertiesLocked") => vec![], // collection, item, lock_metadata, lock_attributes only
        ("nfts", "CollectionLocked") => vec![],     // collection only
        ("nfts", "OwnerChanged") => vec![1],        // new_owner (collection, new_owner)
        ("nfts", "TeamChanged") => vec![],          // collection, issuer, admin, freezer only
        ("nfts", "TransferApproved") => vec![2, 3], // owner, delegate (collection, item, owner, delegate)
        ("nfts", "ApprovalCancelled") => vec![2, 3], // owner, delegate
        ("nfts", "AllApprovalsCancelled") => vec![2], // owner (collection, item, owner)
        ("nfts", "CollectionConfigChanged") => vec![], // collection only
        ("nfts", "CollectionMetadataSet") => vec![], // collection, data only
        ("nfts", "CollectionMetadataCleared") => vec![], // collection only
        ("nfts", "ItemMetadataSet") => vec![],      // collection, item, data only
        ("nfts", "ItemMetadataCleared") => vec![],  // collection, item only
        ("nfts", "Redeposited") => vec![],          // collection, successful_items only
        ("nfts", "AttributeSet") => vec![],         // collection, maybe_item, key, value only
        ("nfts", "AttributeCleared") => vec![],     // collection, maybe_item, key only
        ("nfts", "ItemAttributesApprovalAdded") => vec![2, 3], // owner, delegate
        ("nfts", "ItemAttributesApprovalRemoved") => vec![2, 3], // owner, delegate
        ("nfts", "OwnershipAcceptanceChanged") => vec![0], // who
        ("nfts", "CollectionMaxSupplySet") => vec![], // collection, max_supply only
        ("nfts", "CollectionMintSettingsUpdated") => vec![], // collection only
        ("nfts", "NextCollectionIdIncremented") => vec![], // next_id only
        ("nfts", "ItemPriceSet") => vec![], // collection, item, price, whitelisted_buyer only
        ("nfts", "ItemPriceRemoved") => vec![], // collection, item only
        ("nfts", "ItemBought") => vec![3, 4], // buyer, seller (collection, item, price, seller, buyer)
        ("nfts", "TipSent") => vec![3, 4], // sender, receiver (collection, item, sender, receiver, amount)
        ("nfts", "SwapCreated") => vec![4, 5], // offered_collection, offered_item, desired_collection, desired_item, price, deadline
        ("nfts", "SwapCancelled") => vec![], // offered_collection, offered_item, desired_collection, desired_item only
        ("nfts", "SwapClaimed") => vec![], // sent_collection, sent_item, sent_item_owner, received_collection, received_item, received_item_owner, price, deadline
        ("nfts", "PreSignedAttributesSet") => vec![2], // owner (collection, item, namespace)
        ("nfts", "PalletAttributeSet") => vec![], // collection, item, attribute, value only

        // Preimage pallet events
        ("preimage", "Noted") => vec![],     // hash only
        ("preimage", "Requested") => vec![], // hash only
        ("preimage", "Cleared") => vec![],   // hash only

        // Scheduler pallet events
        ("scheduler", "Scheduled") => vec![], // when, index only
        ("scheduler", "Canceled") => vec![],  // when, index only
        ("scheduler", "Dispatched") => vec![], // task, id, result only
        ("scheduler", "CallUnavailable") => vec![], // task, id only
        ("scheduler", "PeriodicFailed") => vec![], // task, id only
        ("scheduler", "PermanentlyOverweight") => vec![], // task, id only

        // Bounties pallet events
        ("bounties", "BountyProposed") => vec![], // index only
        ("bounties", "BountyRejected") => vec![], // index, bond only
        ("bounties", "BountyBecameActive") => vec![], // index only
        ("bounties", "BountyAwarded") => vec![1], // beneficiary (index, beneficiary)
        ("bounties", "BountyClaimed") => vec![2], // beneficiary (index, payout, beneficiary)
        ("bounties", "BountyCanceled") => vec![], // index only
        ("bounties", "BountyExtended") => vec![], // index only

        // ChildBounties pallet events
        ("childbounties", "Added") => vec![], // parent_index, index only
        ("childbounties", "Awarded") => vec![2], // beneficiary (parent_index, index, beneficiary)
        ("childbounties", "Claimed") => vec![3], // beneficiary (parent_index, index, payout, beneficiary)
        ("childbounties", "Canceled") => vec![], // parent_index, index only

        // Tips pallet events
        ("tips", "NewTip") => vec![],       // hash only
        ("tips", "TipClosing") => vec![],   // hash only
        ("tips", "TipClosed") => vec![1],   // who (hash, who, payout)
        ("tips", "TipRetracted") => vec![], // hash only
        ("tips", "TipSlashed") => vec![1],  // finder (hash, finder, deposit)

        // Recovery pallet events
        ("recovery", "RecoveryCreated") => vec![0], // account
        ("recovery", "RecoveryInitiated") => vec![0, 1], // lost_account, rescuer_account
        ("recovery", "RecoveryVouched") => vec![0, 1], // lost_account, rescuer_account (lost, rescuer, sender)
        ("recovery", "RecoveryClosed") => vec![0, 1],  // lost_account, rescuer_account
        ("recovery", "AccountRecovered") => vec![0, 1], // lost_account, rescuer_account
        ("recovery", "RecoveryRemoved") => vec![0],    // lost_account

        // ElectionProviderMultiPhase pallet events
        ("electionprovidermultiphase", "SolutionStored") => vec![], // compute, origin, prev_ejected only
        ("electionprovidermultiphase", "ElectionFinalized") => vec![], // compute, score only
        ("electionprovidermultiphase", "ElectionFailed") => vec![], // no accounts
        ("electionprovidermultiphase", "Rewarded") => vec![0],      // account
        ("electionprovidermultiphase", "Slashed") => vec![0],       // account
        ("electionprovidermultiphase", "PhaseTransitioned") => vec![], // from, to, round only

        // Crowdloan pallet events
        ("crowdloan", "Created") => vec![],      // para_id only
        ("crowdloan", "Contributed") => vec![0], // who (who, para_id, amount)
        ("crowdloan", "Withdrew") => vec![0],    // who (who, para_id, amount)
        ("crowdloan", "PartiallyRefunded") => vec![], // para_id only
        ("crowdloan", "AllRefunded") => vec![],  // para_id only
        ("crowdloan", "Dissolved") => vec![],    // para_id only
        ("crowdloan", "HandleBidResult") => vec![], // para_id, result only
        ("crowdloan", "Edited") => vec![],       // para_id only
        ("crowdloan", "MemoUpdated") => vec![0], // who (who, para_id, memo)
        ("crowdloan", "AddedToNewRaise") => vec![], // para_id only

        // XcmPallet events
        ("xcmpallet", "Attempted") => vec![], // outcome only
        ("xcmpallet", "Sent") => vec![],      // origin, destination, message, message_id only
        ("xcmpallet", "UnexpectedResponse") => vec![], // origin, query_id only
        ("xcmpallet", "ResponseReady") => vec![], // query_id, response only
        ("xcmpallet", "Notified") => vec![],  // query_id, pallet_index, call_index only
        ("xcmpallet", "NotifyOverweight") => vec![], // query_id, pallet_index, call_index, actual_weight, max_budgeted_weight only
        ("xcmpallet", "NotifyDispatchError") => vec![], // query_id, pallet_index, call_index only
        ("xcmpallet", "NotifyDecodeFailed") => vec![], // query_id, pallet_index, call_index only
        ("xcmpallet", "InvalidResponder") => vec![], // origin, query_id, expected_location only
        ("xcmpallet", "InvalidResponderVersion") => vec![], // origin, query_id only
        ("xcmpallet", "ResponseTaken") => vec![],    // query_id only
        ("xcmpallet", "AssetsTrapped") => vec![],    // hash, origin, assets only
        ("xcmpallet", "VersionChangeNotified") => vec![], // destination, result, cost, message_id only
        ("xcmpallet", "SupportedVersionChanged") => vec![], // location, version only
        ("xcmpallet", "NotifyTargetSendFail") => vec![],  // location, query_id, error only
        ("xcmpallet", "NotifyTargetMigrationFail") => vec![], // location, query_id only
        ("xcmpallet", "InvalidQuerierVersion") => vec![], // origin, query_id only
        ("xcmpallet", "InvalidQuerier") => vec![], // origin, query_id, expected_querier, maybe_actual_querier only
        ("xcmpallet", "VersionNotifyStarted") => vec![], // destination, cost, message_id only
        ("xcmpallet", "VersionNotifyRequested") => vec![], // destination, cost, message_id only
        ("xcmpallet", "VersionNotifyUnrequested") => vec![], // destination, cost, message_id only
        ("xcmpallet", "FeesPaid") => vec![],       // paying, fees only
        ("xcmpallet", "AssetsClaimed") => vec![],  // hash, origin, assets only

        // ParachainSystem events
        ("parachainsystem", "ValidationFunctionStored") => vec![], // no accounts
        ("parachainsystem", "ValidationFunctionApplied") => vec![], // relay_chain_block_num only
        ("parachainsystem", "ValidationFunctionDiscarded") => vec![], // no accounts
        ("parachainsystem", "UpgradeAuthorized") => vec![],        // code_hash only
        ("parachainsystem", "DownwardMessagesReceived") => vec![], // count only
        ("parachainsystem", "DownwardMessagesProcessed") => vec![], // weight_used, dmq_head only
        ("parachainsystem", "UpwardMessageSent") => vec![],        // message_hash only

        // System events
        ("system", "NewAccount") => vec![0], // who

        // DmpQueue events (Downward Message Passing)
        ("dmpqueue", "InvalidFormat") => vec![], // message_id only
        ("dmpqueue", "UnsupportedVersion") => vec![], // message_id only
        ("dmpqueue", "ExecutedDownward") => vec![], // message_id, outcome only
        ("dmpqueue", "WeightExhausted") => vec![], // message_id, remaining_weight, required_weight only
        ("dmpqueue", "OverweightEnqueued") => vec![], // message_id, overweight_index, required_weight only
        ("dmpqueue", "OverweightServiced") => vec![], // overweight_index, weight_used only

        // UmpQueue events (Upward Message Passing)
        ("umpqueue", "ExecutedUpward") => vec![], // message_id, outcome only
        ("umpqueue", "WeightExhausted") => vec![], // message_id, remaining_weight, required_weight only
        ("umpqueue", "UpwardMessagesReceived") => vec![], // from, count only
        ("umpqueue", "OverweightEnqueued") => vec![], // from, message_id, overweight_index, required_weight only
        ("umpqueue", "OverweightServiced") => vec![], // overweight_index, weight_used only

        // XcmpQueue events (Cross-chain Message Passing)
        ("xcmpqueue", "Success") => vec![], // message_hash, weight only
        ("xcmpqueue", "Fail") => vec![],    // message_hash, error, weight only
        ("xcmpqueue", "BadVersion") => vec![], // message_hash only
        ("xcmpqueue", "BadFormat") => vec![], // message_hash only
        ("xcmpqueue", "XcmpMessageSent") => vec![], // message_hash only
        ("xcmpqueue", "OverweightEnqueued") => vec![], // sender, sent_at, index, required only
        ("xcmpqueue", "OverweightServiced") => vec![], // index, used only

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

    #[test]
    fn test_democracy_delegated() {
        assert_eq!(
            get_account_field_positions("democracy", "Delegated"),
            vec![0, 1]
        );
    }

    #[test]
    fn test_nominationpools_bonded() {
        assert_eq!(
            get_account_field_positions("nominationpools", "Bonded"),
            vec![0]
        );
    }

    #[test]
    fn test_assets_transferred() {
        assert_eq!(
            get_account_field_positions("assets", "Transferred"),
            vec![1, 2]
        );
    }

    #[test]
    fn test_nfts_transferred() {
        assert_eq!(
            get_account_field_positions("nfts", "Transferred"),
            vec![2, 3]
        );
    }

    #[test]
    fn test_crowdloan_contributed() {
        assert_eq!(
            get_account_field_positions("crowdloan", "Contributed"),
            vec![0]
        );
    }

    #[test]
    fn test_xcm_no_accounts() {
        assert_eq!(
            get_account_field_positions("xcmpallet", "Attempted"),
            Vec::<usize>::new()
        );
    }
}
