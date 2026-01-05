//! SCALE decoding and JSON transformation for block data.
//!
//! # Why This Module Exists
//!
//! This module handles the **decoding** and **transformation** of SCALE-encoded data
//! into JSON. It is separate from `processing/` because decoding requires specialized
//! visitor patterns and type-aware logic that differs based on the data source:
//!
//! - **Extrinsic args** use `JsonVisitor` (type-aware at decode time)
//! - **Events** use `EventsVisitor` + post-processing transforms (different JSON format)
//! - **XCM messages** use `scale_value` + registry-aware conversion (different decode path)
//!
//! Each decoder produces different JSON output formats to match substrate-api-sidecar's
//! API compatibility requirements.

pub mod args;
pub mod events;
pub mod type_name;
pub mod xcm;

// Re-export commonly used types
pub use args::JsonVisitor;
pub use events::{
    EventField, EventInfo, EventPhase, EventsVisitor, convert_bytes_to_hex, transform_json_unified,
    try_convert_accountid_to_ss58,
};
pub use type_name::GetTypeName;
pub use xcm::XcmDecoder;
