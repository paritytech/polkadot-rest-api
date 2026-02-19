// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests verifying that `deny_unknown_fields` on query parameter structs
//! causes Axum to return 400 Bad Request when unknown query params are sent.
//!
//! These tests exercise the full Axum extraction pipeline: HTTP request → Query<T> extractor
//! → serde deserialization → rejection response. No AppState or RPC connection is needed
//! because the Query extractor rejects the request before the handler body runs.

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::body::Body;
    use axum::extract::Query;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    use axum::routing::get;
    use http_body_util::BodyExt;
    use serde::Deserialize;
    use tower::ServiceExt;

    // ========================================================================
    // Test helpers
    // ========================================================================

    /// Send a GET request to the test router and return (status, body_string).
    async fn send_request(app: Router, uri: &str) -> (StatusCode, String) {
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let text = String::from_utf8_lossy(&body).to_string();
        (status, text)
    }

    // ========================================================================
    // Minimal handler stubs — these never execute when query params are invalid
    // ========================================================================

    /// Stub handler using a struct with `rename_all = "camelCase"` + `deny_unknown_fields`
    #[allow(dead_code)]
    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct CamelCaseParams {
        #[serde(default)]
        pub event_docs: bool,
        #[serde(default)]
        pub no_fees: bool,
        pub at: Option<String>,
    }

    async fn camel_case_handler(Query(_params): Query<CamelCaseParams>) -> impl IntoResponse {
        "ok"
    }

    /// Stub handler using a struct with only `deny_unknown_fields` (no rename_all)
    #[allow(dead_code)]
    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct PlainParams {
        pub at: Option<String>,
    }

    async fn plain_handler(Query(_params): Query<PlainParams>) -> impl IntoResponse {
        "ok"
    }

    /// Stub handler using a struct with `rename_all = "camelCase"` + boolean defaults
    #[allow(dead_code)]
    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct BoolDefaultParams {
        #[serde(default)]
        pub use_rc_block: bool,
        #[serde(default)]
        pub only_ids: bool,
        pub at: Option<String>,
    }

    async fn bool_default_handler(Query(_params): Query<BoolDefaultParams>) -> impl IntoResponse {
        "ok"
    }

    // ========================================================================
    // Tests: unknown fields are rejected with 400 Bad Request
    // ========================================================================

    #[tokio::test]
    async fn unknown_camel_case_param_returns_400() {
        let app = Router::new().route("/test", get(camel_case_handler));

        let (status, body) = send_request(app, "/test?eventDocs=true&badParam=1").await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.contains("unknown field"),
            "Expected 'unknown field' in body, got: {body}"
        );
    }

    #[tokio::test]
    async fn unknown_plain_param_returns_400() {
        let app = Router::new().route("/test", get(plain_handler));

        let (status, body) = send_request(app, "/test?at=100&surprise=yes").await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.contains("unknown field"),
            "Expected 'unknown field' in body, got: {body}"
        );
    }

    #[tokio::test]
    async fn snake_case_rejected_when_camel_case_required() {
        let app = Router::new().route("/test", get(camel_case_handler));

        // "event_docs" is snake_case, should be "eventDocs"
        let (status, body) = send_request(app, "/test?event_docs=true").await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.contains("unknown field"),
            "Expected 'unknown field' in body, got: {body}"
        );
    }

    #[tokio::test]
    async fn misspelled_camel_case_rejected() {
        let app = Router::new().route("/test", get(bool_default_handler));

        // "useRcblock" instead of "useRcBlock"
        let (status, body) = send_request(app, "/test?useRcblock=true").await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.contains("unknown field"),
            "Expected 'unknown field' in body, got: {body}"
        );
    }

    // ========================================================================
    // Tests: valid params still work (200 OK)
    // ========================================================================

    #[tokio::test]
    async fn valid_camel_case_params_return_200() {
        let app = Router::new().route("/test", get(camel_case_handler));

        let (status, _) = send_request(app, "/test?eventDocs=true&noFees=false&at=100").await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn valid_plain_params_return_200() {
        let app = Router::new().route("/test", get(plain_handler));

        let (status, _) = send_request(app, "/test?at=0xabc123").await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn empty_query_string_returns_200() {
        let app = Router::new().route("/test", get(camel_case_handler));

        let (status, _) = send_request(app, "/test").await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn no_query_params_returns_200() {
        let app = Router::new().route("/test", get(bool_default_handler));

        let (status, _) = send_request(app, "/test").await;

        assert_eq!(status, StatusCode::OK);
    }

    // ========================================================================
    // Tests: error message quality
    // ========================================================================

    #[tokio::test]
    async fn error_message_mentions_the_unknown_field_name() {
        let app = Router::new().route("/test", get(camel_case_handler));

        let (status, body) = send_request(app, "/test?fooBar=123").await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.contains("fooBar"),
            "Expected error to mention 'fooBar', got: {body}"
        );
    }

    #[tokio::test]
    async fn error_message_suggests_valid_fields() {
        let app = Router::new().route("/test", get(camel_case_handler));

        let (_, body) = send_request(app, "/test?eventDoc=true").await;

        // serde's deny_unknown_fields error includes "expected one of ..." with valid field names
        assert!(
            body.contains("eventDocs") || body.contains("noFees") || body.contains("at"),
            "Expected error to suggest valid fields, got: {body}"
        );
    }
}
