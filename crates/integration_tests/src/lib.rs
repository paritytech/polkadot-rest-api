// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod client;
pub mod config;
pub mod fixtures;
pub mod utils;

pub use client::TestClient;
pub use config::{ChainConfig, TestConfig};
pub use fixtures::FixtureLoader;
pub use utils::*;

/// Test configuration constants
pub mod constants {
    /// Maximum number of retries when waiting for the API to be ready (in seconds)
    pub const API_READY_TIMEOUT_SECONDS: u32 = 30;
}
