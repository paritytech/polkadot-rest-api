// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Handlers for parachain-related endpoints.

pub mod paras_inclusion;

pub use paras_inclusion::get_paras_inclusion;
