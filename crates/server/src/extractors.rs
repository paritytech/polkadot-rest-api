// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Custom Axum extractors that return JSON error responses.

use axum::Json;
use axum::extract::Query;
use axum::extract::rejection::QueryRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::de::DeserializeOwned;
use serde_json::json;

// ============================================================================
// QsQuery - Query extractor with bracket notation support (keys[]=val)
// ============================================================================

/// A query extractor that supports PHP-style bracket notation for arrays.
///
/// Uses `serde_qs` instead of `serde_urlencoded` to properly parse:
/// - `keys[]=val1&keys[]=val2` -> Vec with ["val1", "val2"]
/// - `keys[0]=val1&keys[1]=val2` -> Vec with ["val1", "val2"]
///
/// This is required for Sidecar API compatibility where storage keys are
/// passed as `?keys[]=key1&keys[]=key2`.
pub struct QsQuery<T>(pub T);

impl<T, S> axum::extract::FromRequestParts<S> for QsQuery<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = Response;

    fn from_request_parts<'life0, 'life1, 'async_trait>(
        parts: &'life0 mut axum::http::request::Parts,
        _state: &'life1 S,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self, Self::Rejection>> + Send + 'async_trait>,
    >
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let raw_query = parts.uri.query().unwrap_or("");

            // Use non-strict mode to handle URL-encoded brackets (%5B%5D)
            // Strict mode requires raw brackets which browsers typically encode
            let config = serde_qs::Config::new(5, false);

            match config.deserialize_str::<T>(raw_query) {
                Ok(value) => Ok(QsQuery(value)),
                Err(e) => Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("Failed to deserialize query string: {}", e) })),
                )
                    .into_response()),
            }
        })
    }
}

// ============================================================================
// JsonQuery - Standard query extractor with JSON error responses
// ============================================================================

/// A wrapper around [`Query<T>`] that returns JSON error responses on rejection.
///
/// Axum's default `Query<T>` returns plain-text errors when deserialization fails
/// (e.g., unknown fields with `deny_unknown_fields`). This extractor converts
/// those rejections to `{"error": "..."}` JSON with 400 Bad Request status.
pub struct JsonQuery<T>(pub T);

impl<T, S> axum::extract::FromRequestParts<S> for JsonQuery<T>
where
    T: DeserializeOwned + Send,
    S: Send + Sync,
{
    type Rejection = Response;

    fn from_request_parts<'life0, 'life1, 'async_trait>(
        parts: &'life0 mut axum::http::request::Parts,
        state: &'life1 S,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self, Self::Rejection>> + Send + 'async_trait>,
    >
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            match Query::<T>::from_request_parts(parts, state).await {
                Ok(Query(value)) => Ok(JsonQuery(value)),
                Err(rejection) => Err(json_query_error(rejection)),
            }
        })
    }
}

fn json_query_error(rejection: QueryRejection) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": rejection.body_text() })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::routing::get;
    use http_body_util::BodyExt;
    use serde::Deserialize;
    use tower::ServiceExt;

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct TestParams {
        #[serde(default)]
        pub event_docs: bool,
        pub at: Option<String>,
    }

    async fn test_handler(JsonQuery(_params): JsonQuery<TestParams>) -> &'static str {
        "ok"
    }

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

    #[tokio::test]
    async fn valid_params_return_200() {
        let app = Router::new().route("/test", get(test_handler));
        let (status, _) = send_request(app, "/test?eventDocs=true&at=100").await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn unknown_field_returns_json_400() {
        let app = Router::new().route("/test", get(test_handler));
        let (status, body) = send_request(app, "/test?badParam=1").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let parsed: serde_json::Value =
            serde_json::from_str(&body).expect("Response should be valid JSON");
        let error_msg = parsed["error"].as_str().unwrap();
        assert!(
            error_msg.contains("unknown field") || error_msg.contains("badParam"),
            "Error message should mention unknown field or the bad param name, got: {error_msg}"
        );
    }

    #[tokio::test]
    async fn empty_query_returns_200() {
        let app = Router::new().route("/test", get(test_handler));
        let (status, _) = send_request(app, "/test").await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn error_is_json_not_plain_text() {
        let app = Router::new().route("/test", get(test_handler));
        let (_, body) = send_request(app, "/test?foo=bar").await;
        // Verify it's valid JSON with an "error" key
        let parsed: serde_json::Value =
            serde_json::from_str(&body).expect("Response must be valid JSON, not plain text");
        assert!(parsed.get("error").is_some());
    }
}
