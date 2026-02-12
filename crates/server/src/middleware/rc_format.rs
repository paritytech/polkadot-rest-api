use axum::{
    Json,
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use http_body_util::BodyExt;
use serde::Deserialize;
use serde_json::json;

use crate::state::AppState;

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct RcFormatParams {
    #[serde(default)]
    format: Option<String>,
    #[serde(default)]
    use_rc_block: Option<String>,
    #[serde(default)]
    at: Option<String>,
}

/// Middleware that transforms RC block array responses into a structured object
/// when `format=rc` is present in the query string.
///
/// Requires `useRcBlock=true` to be present alongside `format=rc`.
/// Returns 400 Bad Request if `format=rc` is used without `useRcBlock=true`.
pub async fn rc_format_middleware(
    State(state): State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let params = req
        .uri()
        .query()
        .and_then(|q| serde_urlencoded::from_str::<RcFormatParams>(q).ok())
        .unwrap_or_default();

    let has_format_rc = params.format.as_deref() == Some("rc");

    if !has_format_rc {
        return next.run(req).await;
    }

    let has_use_rc_block = params.use_rc_block.as_deref() == Some("true");
    if !has_use_rc_block {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "format=rc requires useRcBlock=true" })),
        )
            .into_response();
    }

    let at_param = params.at;

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

    let (mut parts, body) = response.into_parts();
    let bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => return Response::from_parts(parts, Body::empty()),
    };

    let transformed = match transform_rc_response(&bytes, &state, at_param.as_deref()).await {
        Some(new_bytes) => new_bytes,
        None => return Response::from_parts(parts, Body::from(bytes)),
    };

    parts.headers.remove(axum::http::header::CONTENT_LENGTH);
    Response::from_parts(parts, Body::from(transformed))
}

async fn transform_rc_response(
    bytes: &[u8],
    state: &AppState,
    at_param: Option<&str>,
) -> Option<Vec<u8>> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;

    if let serde_json::Value::Array(arr) = &value {
        if arr.is_empty() {
            let rc_block_info = if let Some(at) = at_param {
                fetch_rc_block_info(state, at).await
            } else {
                None
            };
            let result = serde_json::json!({
                "rcBlock": rc_block_info,
                "parachainDataPerBlock": []
            });
            return serde_json::to_vec(&result).ok();
        }
    }

    let rc_hash = extract_rc_hash(&value)?;
    let parent_hash = fetch_rc_parent_hash(state, rc_hash).await;
    wrap_rc_response(value, parent_hash)
}

/// Extract `rcBlockHash` from the first element (array) or the object itself.
fn extract_rc_hash(value: &serde_json::Value) -> Option<&str> {
    match value {
        serde_json::Value::Array(arr) => arr.first()?.as_object()?.get("rcBlockHash")?.as_str(),
        serde_json::Value::Object(obj) => obj.get("rcBlockHash")?.as_str(),
        _ => None,
    }
}

/// Wrap a JSON response in the `{ rcBlock, parachainDataPerBlock }` structure.
fn wrap_rc_response(value: serde_json::Value, parent_hash: Option<String>) -> Option<Vec<u8>> {
    match &value {
        serde_json::Value::Array(arr) => {
            let first = arr.first()?.as_object()?;
            let rc_hash = first.get("rcBlockHash")?.as_str()?;
            let rc_number = first.get("rcBlockNumber")?.as_str()?;
            let rc_block = build_rc_block(rc_hash, rc_number, parent_hash);

            let result = serde_json::json!({
                "rcBlock": rc_block,
                "parachainDataPerBlock": arr,
            });

            serde_json::to_vec(&result).ok()
        }
        serde_json::Value::Object(obj) => {
            let rc_hash = obj.get("rcBlockHash")?.as_str()?;
            let rc_number = obj.get("rcBlockNumber")?.as_str()?;
            let rc_block = build_rc_block(rc_hash, rc_number, parent_hash);

            let result = serde_json::json!({
                "rcBlock": rc_block,
                "parachainDataPerBlock": [value],
            });

            serde_json::to_vec(&result).ok()
        }
        _ => None,
    }
}

fn build_rc_block(
    rc_hash: &str,
    rc_number: &str,
    parent_hash: Option<String>,
) -> serde_json::Map<String, serde_json::Value> {
    let mut rc_block = serde_json::Map::new();
    rc_block.insert("hash".to_string(), serde_json::json!(rc_hash));
    rc_block.insert(
        "parentHash".to_string(),
        parent_hash
            .map(|ph| serde_json::json!(ph))
            .unwrap_or(serde_json::Value::Null),
    );
    rc_block.insert("number".to_string(), serde_json::json!(rc_number));
    rc_block
}

async fn fetch_rc_parent_hash(state: &AppState, rc_block_hash: &str) -> Option<String> {
    let relay_rpc = state.get_relay_chain_rpc()?;
    let hash: subxt::utils::H256 = rc_block_hash.parse().ok()?;
    let header = relay_rpc.chain_get_header(Some(hash)).await.ok()??;
    Some(format!("{:#x}", header.parent_hash))
}

/// Fetch full RC block info (hash, parentHash, number) from an `at` parameter.
async fn fetch_rc_block_info(
    state: &AppState,
    at: &str,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let relay_rpc = state.get_relay_chain_rpc()?;

    let hash: subxt::utils::H256 = if at.starts_with("0x") {
        at.parse().ok()?
    } else {
        let number: u32 = at.parse().ok()?;
        relay_rpc
            .chain_get_block_hash(Some(number.into()))
            .await
            .ok()??
    };

    let header = relay_rpc.chain_get_header(Some(hash)).await.ok()??;
    let mut rc_block = serde_json::Map::new();
    rc_block.insert("hash".to_string(), json!(format!("{:#x}", hash)));
    rc_block.insert(
        "parentHash".to_string(),
        json!(format!("{:#x}", header.parent_hash)),
    );
    rc_block.insert("number".to_string(), json!(header.number.to_string()));
    Some(rc_block)
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
    /// Uses `wrap_rc_response` with no parentHash (no RPC available in tests).
    async fn test_rc_format(req: Request, next: Next) -> Response {
        let params = req
            .uri()
            .query()
            .and_then(|q| serde_urlencoded::from_str::<RcFormatParams>(q).ok())
            .unwrap_or_default();

        let has_format_rc = params.format.as_deref() == Some("rc");

        if !has_format_rc {
            return next.run(req).await;
        }

        let has_use_rc_block = params.use_rc_block.as_deref() == Some("true");
        if !has_use_rc_block {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "format=rc requires useRcBlock=true" })),
            )
                .into_response();
        }

        let response = next.run(req).await;
        if !response.status().is_success() {
            return response;
        }

        let (mut parts, body) = response.into_parts();
        let bytes = match body.collect().await {
            Ok(collected) => collected.to_bytes(),
            Err(_) => return Response::from_parts(parts, Body::empty()),
        };

        let value: serde_json::Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            Err(_) => return Response::from_parts(parts, Body::from(bytes)),
        };

        if let serde_json::Value::Array(arr) = &value {
            if arr.is_empty() {
                let result = serde_json::json!({
                    "rcBlock": null,
                    "parachainDataPerBlock": []
                });
                if let Ok(new_bytes) = serde_json::to_vec(&result) {
                    parts.headers.remove(axum::http::header::CONTENT_LENGTH);
                    return Response::from_parts(parts, Body::from(new_bytes));
                }
            }
        }

        match wrap_rc_response(value, None) {
            Some(new_bytes) => {
                parts.headers.remove(axum::http::header::CONTENT_LENGTH);
                Response::from_parts(parts, Body::from(new_bytes))
            }
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

        let (status, value) = make_request(app, "/test?useRcBlock=true&format=rc").await;

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

        let (status, value) = make_request(app, "/test?useRcBlock=true&format=rc").await;

        assert_eq!(status, StatusCode::OK);
        assert!(value["rcBlock"].is_null());
        assert_eq!(value["parachainDataPerBlock"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn single_object_without_rc_fields_passes_through() {
        async fn handler() -> axum::response::Json<serde_json::Value> {
            json_response(serde_json::json!({
                "at": { "hash": "0xabc", "height": "100" },
                "balance": "1000"
            }))
        }

        let app = Router::new()
            .route("/test", get(handler))
            .layer(middleware::from_fn(test_rc_format));

        let (status, value) = make_request(app, "/test?useRcBlock=true&format=rc").await;

        assert_eq!(status, StatusCode::OK);
        assert!(value.is_object());
        assert_eq!(value["balance"], "1000");
    }

    #[tokio::test]
    async fn single_object_with_rc_fields_transforms() {
        async fn handler() -> axum::response::Json<serde_json::Value> {
            json_response(serde_json::json!({
                "at": { "hash": "0xabc", "height": "100" },
                "rcBlockHash": "0xdef",
                "rcBlockNumber": "999",
                "pallet": "balances",
                "items": []
            }))
        }

        let app = Router::new()
            .route("/test", get(handler))
            .layer(middleware::from_fn(test_rc_format));

        let (status, value) = make_request(app, "/test?useRcBlock=true&format=rc").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["rcBlock"]["hash"], "0xdef");
        assert_eq!(value["rcBlock"]["number"], "999");

        let data = value["parachainDataPerBlock"].as_array().unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(data[0]["pallet"], "balances");
        assert_eq!(data[0]["rcBlockHash"], "0xdef");
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
                    .uri("/test?useRcBlock=true&format=rc")
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

        let (status, value) = make_request(app, "/test?useRcBlock=true&format=rc").await;

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

        let (_, value) = make_request(app, "/test?useRcBlock=true&format=rc").await;

        let data = value["parachainDataPerBlock"].as_array().unwrap();
        assert_eq!(data[0]["ahTimestamp"], "123");
        assert_eq!(data[1]["ahTimestamp"], "456");
        assert!(value["rcBlock"].get("ahTimestamp").is_none());
    }

    #[tokio::test]
    async fn format_rc_without_use_rc_block_returns_400() {
        async fn handler() -> axum::response::Json<serde_json::Value> {
            json_response(rc_array())
        }

        let app = Router::new()
            .route("/test", get(handler))
            .layer(middleware::from_fn(test_rc_format));

        let (status, value) = make_request(app, "/test?format=rc").await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(value["error"], "format=rc requires useRcBlock=true");
    }

    #[tokio::test]
    async fn format_rc_with_other_params() {
        async fn handler() -> axum::response::Json<serde_json::Value> {
            json_response(rc_array())
        }

        let app = Router::new()
            .route("/test", get(handler))
            .layer(middleware::from_fn(test_rc_format));

        let (status, value) = make_request(app, "/test?useRcBlock=true&format=rc&at=123").await;

        assert_eq!(status, StatusCode::OK);
        assert!(value.get("rcBlock").is_some());
        assert!(value.get("parachainDataPerBlock").is_some());
    }
}
