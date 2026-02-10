use axum::{
    body::Body,
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use http_body_util::BodyExt;
use serde::Deserialize;

use crate::state::AppState;

#[derive(Deserialize, Default)]
struct FormatParams {
    #[serde(default)]
    format: Option<String>,
}

/// Middleware that transforms RC block array responses into a structured object
/// when `format=rc` is present in the query string.
pub async fn rc_format_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let has_format_rc = req
        .uri()
        .query()
        .and_then(|q| serde_urlencoded::from_str::<FormatParams>(q).ok())
        .is_some_and(|p| p.format.as_deref() == Some("rc"));

    if !has_format_rc {
        return next.run(req).await;
    }

    let response = next.run(req).await;

    if !response.status().is_success() {
        return response;
    }

    let is_json = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.contains("application/json"));

    if !is_json {
        return response;
    }

    let (parts, body) = response.into_parts();
    let bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => return Response::from_parts(parts, Body::empty()),
    };

    let transformed = match transform_rc_response(&bytes, &state).await {
        Some(new_bytes) => new_bytes,
        None => return Response::from_parts(parts, Body::from(bytes)),
    };

    Response::from_parts(parts, Body::from(transformed))
}

async fn transform_rc_response(bytes: &[u8], state: &AppState) -> Option<Vec<u8>> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;

    let arr = value.as_array()?;

    if arr.is_empty() {
        let result = serde_json::json!({
            "rcBlock": null,
            "parachainDataPerBlock": []
        });
        return serde_json::to_vec(&result).ok();
    }

    let first = arr.first()?.as_object()?;
    let rc_hash = first.get("rcBlockHash")?.as_str()?;
    let rc_number = first.get("rcBlockNumber")?.as_str()?;

    let parent_hash = fetch_rc_parent_hash(state, rc_hash).await;

    let mut rc_block = serde_json::Map::new();
    rc_block.insert("hash".to_string(), serde_json::json!(rc_hash));
    if let Some(ph) = parent_hash {
        rc_block.insert("parentHash".to_string(), serde_json::json!(ph));
    }
    rc_block.insert("number".to_string(), serde_json::json!(rc_number));

    let parachain_data: Vec<serde_json::Value> = arr.to_vec();

    let result = serde_json::json!({
        "rcBlock": rc_block,
        "parachainDataPerBlock": parachain_data,
    });

    serde_json::to_vec(&result).ok()
}

async fn fetch_rc_parent_hash(state: &AppState, rc_block_hash: &str) -> Option<String> {
    let rpc_client = state.get_relay_chain_rpc_client()?;
    let header_json: serde_json::Value = rpc_client
        .request("chain_getHeader", subxt_rpcs::rpc_params![rc_block_hash])
        .await
        .ok()?;
    header_json
        .get("parentHash")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

#[cfg(test)]
fn transform_rc_response_stateless(bytes: &[u8]) -> Option<Vec<u8>> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;

    let arr = value.as_array()?;

    if arr.is_empty() {
        let result = serde_json::json!({
            "rcBlock": null,
            "parachainDataPerBlock": []
        });
        return serde_json::to_vec(&result).ok();
    }

    let first = arr.first()?.as_object()?;
    let rc_hash = first.get("rcBlockHash")?.as_str()?;
    let rc_number = first.get("rcBlockNumber")?.as_str()?;

    let rc_block = serde_json::json!({
        "hash": rc_hash,
        "number": rc_number,
    });

    let result = serde_json::json!({
        "rcBlock": rc_block,
        "parachainDataPerBlock": arr,
    });

    serde_json::to_vec(&result).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use axum::{Router, middleware, routing::get};
    use tower::ServiceExt;

    fn json_response(body: serde_json::Value) -> axum::response::Json<serde_json::Value> {
        axum::response::Json(body)
    }

    async fn make_request(app: Router, uri: &str) -> (StatusCode, serde_json::Value) {
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
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        (status, value)
    }

    /// Test middleware that mirrors rc_format_middleware but without AppState.
    /// Uses transform_rc_response_stateless (no parentHash in rcBlock).
    async fn test_rc_format(req: Request, next: Next) -> Response {
        let has_format_rc = req
            .uri()
            .query()
            .and_then(|q| serde_urlencoded::from_str::<FormatParams>(q).ok())
            .is_some_and(|p| p.format.as_deref() == Some("rc"));

        if !has_format_rc {
            return next.run(req).await;
        }

        let response = next.run(req).await;
        if !response.status().is_success() {
            return response;
        }

        let (parts, body) = response.into_parts();
        let bytes = match body.collect().await {
            Ok(collected) => collected.to_bytes(),
            Err(_) => return Response::from_parts(parts, Body::empty()),
        };

        match transform_rc_response_stateless(&bytes) {
            Some(new_bytes) => Response::from_parts(parts, Body::from(new_bytes)),
            None => Response::from_parts(parts, Body::from(bytes)),
        }
    }

    fn rc_array() -> serde_json::Value {
        serde_json::json!([
            {
                "at": { "hash": "0xabc", "height": "100" },
                "rcBlockHash": "0xdef",
                "rcBlockNumber": "999",
                "ahTimestamp": "123",
                "someData": "foo"
            },
            {
                "at": { "hash": "0xghi", "height": "101" },
                "rcBlockHash": "0xdef",
                "rcBlockNumber": "999",
                "ahTimestamp": "456",
                "someData": "bar"
            }
        ])
    }

    #[tokio::test]
    async fn happy_path_transforms_rc_array() {
        async fn handler() -> axum::response::Json<serde_json::Value> {
            json_response(rc_array())
        }

        let app = Router::new()
            .route("/test", get(handler))
            .layer(middleware::from_fn(test_rc_format));

        let (status, value) = make_request(app, "/test?format=rc").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["rcBlock"]["hash"], "0xdef");
        assert_eq!(value["rcBlock"]["number"], "999");

        let data = value["parachainDataPerBlock"].as_array().unwrap();
        assert_eq!(data.len(), 2);
        assert_eq!(data[0]["ahTimestamp"], "123");
        assert_eq!(data[0]["someData"], "foo");
        assert_eq!(data[0]["rcBlockHash"], "0xdef");
        assert_eq!(data[0]["rcBlockNumber"], "999");
        assert_eq!(data[1]["ahTimestamp"], "456");
        assert_eq!(data[1]["someData"], "bar");
    }

    #[tokio::test]
    async fn no_format_param_passes_through() {
        async fn handler() -> axum::response::Json<serde_json::Value> {
            json_response(rc_array())
        }

        let app = Router::new()
            .route("/test", get(handler))
            .layer(middleware::from_fn(test_rc_format));

        let (status, value) = make_request(app, "/test").await;

        assert_eq!(status, StatusCode::OK);
        assert!(value.is_array());
        assert_eq!(value.as_array().unwrap().len(), 2);
        assert_eq!(value[0]["rcBlockHash"], "0xdef");
    }

    #[tokio::test]
    async fn empty_array_returns_null_rc_block() {
        async fn handler() -> axum::response::Json<serde_json::Value> {
            json_response(serde_json::json!([]))
        }

        let app = Router::new()
            .route("/test", get(handler))
            .layer(middleware::from_fn(test_rc_format));

        let (status, value) = make_request(app, "/test?format=rc").await;

        assert_eq!(status, StatusCode::OK);
        assert!(value["rcBlock"].is_null());
        assert_eq!(value["parachainDataPerBlock"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn single_object_passes_through() {
        async fn handler() -> axum::response::Json<serde_json::Value> {
            json_response(serde_json::json!({
                "at": { "hash": "0xabc", "height": "100" },
                "balance": "1000"
            }))
        }

        let app = Router::new()
            .route("/test", get(handler))
            .layer(middleware::from_fn(test_rc_format));

        let (status, value) = make_request(app, "/test?format=rc").await;

        assert_eq!(status, StatusCode::OK);
        assert!(value.is_object());
        assert_eq!(value["balance"], "1000");
    }

    #[tokio::test]
    async fn error_response_passes_through() {
        async fn handler() -> (StatusCode, axum::response::Json<serde_json::Value>) {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json_response(serde_json::json!({ "error": "something went wrong" })),
            )
        }

        let app = Router::new()
            .route("/test", get(handler))
            .layer(middleware::from_fn(test_rc_format));

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/test?format=rc")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn array_without_rc_fields_passes_through() {
        async fn handler() -> axum::response::Json<serde_json::Value> {
            json_response(serde_json::json!([
                { "at": { "hash": "0xabc" }, "data": "foo" },
                { "at": { "hash": "0xdef" }, "data": "bar" }
            ]))
        }

        let app = Router::new()
            .route("/test", get(handler))
            .layer(middleware::from_fn(test_rc_format));

        let (status, value) = make_request(app, "/test?format=rc").await;

        assert_eq!(status, StatusCode::OK);
        assert!(value.is_array());
        assert_eq!(value.as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn ah_timestamp_preserved_on_elements() {
        async fn handler() -> axum::response::Json<serde_json::Value> {
            json_response(rc_array())
        }

        let app = Router::new()
            .route("/test", get(handler))
            .layer(middleware::from_fn(test_rc_format));

        let (_, value) = make_request(app, "/test?format=rc").await;

        let data = value["parachainDataPerBlock"].as_array().unwrap();
        assert_eq!(data[0]["ahTimestamp"], "123");
        assert_eq!(data[1]["ahTimestamp"], "456");
        assert!(value["rcBlock"].get("ahTimestamp").is_none());
    }

    #[tokio::test]
    async fn format_rc_with_other_params() {
        async fn handler() -> axum::response::Json<serde_json::Value> {
            json_response(rc_array())
        }

        let app = Router::new()
            .route("/test", get(handler))
            .layer(middleware::from_fn(test_rc_format));

        let (status, value) =
            make_request(app, "/test?useRcBlock=true&format=rc&at=123").await;

        assert_eq!(status, StatusCode::OK);
        assert!(value.get("rcBlock").is_some());
        assert!(value.get("parachainDataPerBlock").is_some());
    }
}
