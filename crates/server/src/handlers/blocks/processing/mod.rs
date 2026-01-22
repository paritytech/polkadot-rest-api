//! Block data processing and extraction.
//!
//! # Why This Module Exists
//!
//! This module handles the **fetching**, **extraction**, and **categorization** of block
//! data from the chain. It is separate from `decode/` because these functions operate
//! at a higher level - they orchestrate the decoding process and organize the results
//! into structured responses.
//!
//! The separation follows this principle:
//! - `decode/` = HOW to decode SCALE data into JSON (visitor patterns, transformations)
//! - `processing/` = WHAT to fetch and how to organize it (storage queries, categorization)

pub mod events;
pub mod extrinsics;
pub mod fees;

pub use events::{
    categorize_events, extract_class_from_event_data, extract_fee_from_transaction_paid_event,
    extract_pays_fee_from_event_data, extract_weight_from_event_data, fetch_block_events,
    fetch_block_events_with_client, fetch_block_events_with_prefix,
};
pub use extrinsics::{extract_extrinsics, extract_extrinsics_with_client, extract_extrinsics_with_prefix};
pub use fees::{extract_fee_info_for_extrinsic, extract_fee_info_for_extrinsic_with_client, get_query_info};
