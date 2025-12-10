use axum::{
    body::Body,
    extract::{MatchedPath, Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use http_body_util::BodyExt;
use std::time::Instant;

use crate::state::AppState;

use super::registry::{
    HTTP_REQUEST_ERROR, HTTP_REQUEST_SUCCESS, HTTP_REQUESTS, REQUEST_DURATION_SECONDS,
    RESPONSE_SIZE_BYTES, RESPONSE_SIZE_BYTES_SECONDS,
};

/// Normalize a route path by replacing parameters with :paramName
/// Example: /blocks/12345 -> /blocks/:blockId
///
/// If include_query_params is true and query_string is provided:
/// /blocks/:blockId?finalized=<?>&eventDocs=<?>
fn normalize_route(path: &str, query_string: Option<&str>, include_query_params: bool) -> String {
    // Common parameter patterns (use $ to match end of string)
    let patterns = vec![
        // Block IDs (numbers or hashes)
        (r"/blocks/[0-9]+$", "/blocks/:blockId"),
        (r"/blocks/0x[a-fA-F0-9]+$", "/blocks/:blockId"),
        // Add more patterns as needed for other endpoints
    ];

    let mut normalized = path.to_string();
    for (pattern, replacement) in patterns {
        if let Ok(re) = regex::Regex::new(pattern)
            && re.is_match(&normalized)
        {
            normalized = re.replace(&normalized, replacement).to_string();
            break;
        }
    }

    // Add query parameters if enabled
    if include_query_params
        && let Some(query) = query_string
        && !query.is_empty()
    {
        // Parse query string and extract parameter names
        let mut params: Vec<String> = query
            .split('&')
            .filter_map(|pair| pair.split('=').next().map(|name| name.to_string()))
            .collect();

        // Sort alphabetically (matches sidecar behavior)
        params.sort();

        // Build query string with <?> placeholders
        let query_params = params
            .iter()
            .map(|name| format!("{}=<?>", name))
            .collect::<Vec<_>>()
            .join("&");

        normalized = format!("{}?{}", normalized, query_params);
    }

    normalized
}

/// Metrics middleware for tracking HTTP requests
pub async fn metrics_middleware(
    State(state): State<AppState>,
    matched_path: Option<MatchedPath>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Skip metrics endpoint itself
    let path = req.uri().path();
    if path == "/metrics" || path == "/metrics.json" {
        return Ok(next.run(req).await);
    }

    // Increment total requests counter
    HTTP_REQUESTS.inc();

    // Start timer for request duration
    let start = Instant::now();

    // Get method, query string, and route
    let method = req.method().to_string();
    let query_string = req.uri().query();
    let include_query_params = state.config.metrics.include_queryparams;

    let route = matched_path
        .as_ref()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| normalize_route(path, query_string, include_query_params));

    // Process request
    let response = next.run(req).await;

    // Calculate duration
    let duration = start.elapsed().as_secs_f64();

    // Get status code
    let status = response.status();
    let status_code = status.as_u16().to_string();

    // Record success/error counters
    if status.is_client_error() || status.is_server_error() {
        HTTP_REQUEST_ERROR.inc();
    } else if status.is_success() {
        HTTP_REQUEST_SUCCESS.inc();
    }

    // Record request duration
    REQUEST_DURATION_SECONDS
        .with_label_values(&[&method, &route, &status_code])
        .observe(duration);

    // Collect the response body to measure its size
    let (parts, body) = response.into_parts();
    let bytes = body
        .collect()
        .await
        .map(|collected| collected.to_bytes())
        .unwrap_or_default();
    let response_size = bytes.len() as f64;

    if response_size > 0.0 {
        // Record response size
        RESPONSE_SIZE_BYTES
            .with_label_values(&[&method, &route, &status_code])
            .observe(response_size);

        // Record response size to latency ratio
        if duration > 0.0 {
            let ratio = response_size / duration;
            RESPONSE_SIZE_BYTES_SECONDS
                .with_label_values(&[&method, &route, &status_code])
                .observe(ratio);
        }
    }

    // Reconstruct the response with the collected body
    Ok(Response::from_parts(parts, Body::from(bytes)))
}

/// Extension trait for recording block-specific metrics in handlers
pub trait BlockMetrics {
    fn record_block_metrics(
        &self,
        method: &str,
        route: &str,
        status_code: u16,
        total_extrinsics: usize,
        total_blocks: usize,
        duration_secs: f64,
    );
}

/// Dummy implementation - handlers can use this to record block metrics
pub struct MetricsRecorder;

impl BlockMetrics for MetricsRecorder {
    fn record_block_metrics(
        &self,
        method: &str,
        route: &str,
        status_code: u16,
        total_extrinsics: usize,
        total_blocks: usize,
        duration_secs: f64,
    ) {
        use super::registry::{
            EXTRINSICS_IN_REQUEST, EXTRINSICS_PER_BLOCK, EXTRINSICS_PER_SECOND, SECONDS_PER_BLOCK,
        };

        let status_code_str = status_code.to_string();

        // Record total extrinsics in request
        EXTRINSICS_IN_REQUEST
            .with_label_values(&[method, route, &status_code_str])
            .observe(total_extrinsics as f64);

        // Record extrinsics per second
        if duration_secs > 0.0 {
            EXTRINSICS_PER_SECOND
                .with_label_values(&[method, route, &status_code_str])
                .observe(total_extrinsics as f64 / duration_secs);
        }

        // Record extrinsics per block
        if total_blocks > 0 {
            EXTRINSICS_PER_BLOCK
                .with_label_values(&[method, route, &status_code_str])
                .observe(total_extrinsics as f64 / total_blocks as f64);

            // Record seconds per block
            SECONDS_PER_BLOCK
                .with_label_values(&[method, route, &status_code_str])
                .observe(duration_secs / total_blocks as f64);
        }
    }
}
