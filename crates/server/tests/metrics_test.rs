// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use server::metrics;

#[test]
fn test_metrics_initialization() {
    // Initialize metrics with test prefix
    metrics::init("test");

    // Gather metrics - should not panic
    let result = metrics::gather_metrics();
    assert!(result.is_ok());

    let metrics_text = result.unwrap();

    // Should contain at least some metrics
    assert!(!metrics_text.is_empty());

    // Should contain the test_ prefix
    assert!(
        metrics_text.contains("test_"),
        "Metrics should contain 'test_' prefix"
    );

    // Should contain our counter metrics with prefix
    // Counters always appear even with zero values
    assert!(
        metrics_text.contains("test_http_requests"),
        "Should contain test_http_requests"
    );
    assert!(
        metrics_text.contains("test_http_request_success"),
        "Should contain test_http_request_success"
    );
    assert!(
        metrics_text.contains("test_http_request_error"),
        "Should contain test_http_request_error"
    );

    // Note: Histograms won't appear until data is recorded
    // This is normal Prometheus behavior
}

#[test]
fn test_http_metrics_increment() {
    use server::metrics::registry::{HTTP_REQUEST_ERROR, HTTP_REQUEST_SUCCESS, HTTP_REQUESTS};

    // Initialize metrics with test prefix
    metrics::init("test");

    // Get initial values
    let initial_total = HTTP_REQUESTS.get();
    let initial_success = HTTP_REQUEST_SUCCESS.get();
    let initial_error = HTTP_REQUEST_ERROR.get();

    // Increment counters
    HTTP_REQUESTS.inc();
    HTTP_REQUEST_SUCCESS.inc();
    HTTP_REQUEST_ERROR.inc();

    // Verify increments
    assert_eq!(HTTP_REQUESTS.get(), initial_total + 1.0);
    assert_eq!(HTTP_REQUEST_SUCCESS.get(), initial_success + 1.0);
    assert_eq!(HTTP_REQUEST_ERROR.get(), initial_error + 1.0);
}

#[test]
fn test_histogram_metrics() {
    use server::metrics::registry::REQUEST_DURATION_SECONDS;

    // Initialize metrics with test prefix
    metrics::init("test");

    // Record some observations
    REQUEST_DURATION_SECONDS
        .with_label_values(&["GET", "/test", "200"])
        .observe(0.5);

    REQUEST_DURATION_SECONDS
        .with_label_values(&["POST", "/test", "201"])
        .observe(1.2);

    // Gather metrics
    let metrics_text = metrics::gather_metrics().unwrap();

    // Should contain histogram data with test prefix
    assert!(metrics_text.contains("test_request_duration_seconds"));
    assert!(metrics_text.contains("bucket"));
}
