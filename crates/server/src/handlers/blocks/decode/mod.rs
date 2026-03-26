// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! SCALE decoding and JSON transformation for block data.
//!
//! # Why This Module Exists
//!
//! This module handles the **decoding** and **transformation** of SCALE-encoded data
//! into JSON. It is separate from `processing/` because decoding requires specialized
//! visitor patterns and type-aware logic that differs based on the data source:
//!
//! - **Extrinsic args** use `JsonVisitor`/`CallArgsVisitor` (type-aware at decode time)
//! - **Events** use `EventsVisitor` + `EventJsonVisitor` (type-aware at decode time)
//! - **XCM messages** use `scale_value` + registry-aware conversion (different decode path)
//!
//! All decoders use `ScaleVisitor` from `args.rs` for type-aware JSON serialization,
//! with different const generic parameters for field casing and enum variant handling.

pub mod args;
pub mod events;
pub mod type_name;
pub mod xcm;

// Re-export commonly used types
pub use args::{EventJsonVisitor, JsonVisitor};
pub use events::{EventInfo, EventPhase, EventsVisitor};
pub use type_name::GetTypeName;
pub use xcm::XcmDecoder;
