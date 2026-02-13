// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use axum::{extract::Request, middleware::Next, response::Response};
use std::time::Instant;

/// HTTP logger middleware that logs request method, path, status code, and duration.
///
/// - Logs with INFO level (target: http) for 2xx/3xx responses
/// - Logs with WARN level for 4xx responses
/// - Logs with ERROR level for 5xx responses
///
/// Log format: "METHOD /path STATUS DURATIONms"
/// Example: "GET /api/blocks/latest 200 45ms"
pub async fn http_logger_middleware(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let query = req.uri().query().map(|q| format!("?{}", q));
    let start = Instant::now();

    // Process the request
    let response = next.run(req).await;

    // Calculate duration
    let duration = start.elapsed();
    let duration_ms = duration.as_millis();

    // Get status code
    let status = response.status();
    let status_code = status.as_u16();

    // Construct full path with query string if present
    let full_path = if let Some(q) = query {
        format!("{}{}", path, q)
    } else {
        path
    };

    // Emit tracing event based on status code
    match status_code {
        200..=399 => {
            tracing::debug!(
                target: "http",
                method = %method,
                path = %full_path,
                status = status_code,
                duration_ms = duration_ms,
                "{} {} {} {}ms",
                method,
                full_path,
                status_code,
                duration_ms
            );
        }
        400..=499 => {
            tracing::warn!(
                target: "http",
                method = %method,
                path = %full_path,
                status = status_code,
                duration_ms = duration_ms,
                "{} {} {} {}ms",
                method,
                full_path,
                status_code,
                duration_ms
            );
        }
        _ => {
            tracing::error!(
                target: "http",
                method = %method,
                path = %full_path,
                status = status_code,
                duration_ms = duration_ms,
                "{} {} {} {}ms",
                method,
                full_path,
                status_code,
                duration_ms
            );
        }
    }

    response
}
