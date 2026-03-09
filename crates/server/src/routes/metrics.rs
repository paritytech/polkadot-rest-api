// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::{handlers::metrics, state::AppState};
use axum::{Router, routing::get};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/metrics", get(metrics::get_metrics))
        .route("/metrics.json", get(metrics::get_metrics_json))
}
