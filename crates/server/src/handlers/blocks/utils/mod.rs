//! Utility functions for block data processing.
//!
//! This module provides:
//! - `numeric` - Numeric extraction from JSON values
//! - `fee` - Fee information transformation

pub mod fee;
pub mod numeric;

// Re-export commonly used functions
pub use fee::{actual_weight_to_json, transform_fee_info};
pub use numeric::{extract_number_as_string, extract_numeric_string};
