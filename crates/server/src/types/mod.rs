//! Common type wrappers for API responses
//!
//! This module contains newtype wrappers around primitive types to provide
//! consistent formatting and serialization across the API.

pub mod hash;

pub use hash::BlockHash;
