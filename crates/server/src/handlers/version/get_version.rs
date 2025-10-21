use axum::{http::StatusCode, response::Json};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct VersionResponse {
    pub version: String,
}

pub async fn get_version() -> (StatusCode, Json<VersionResponse>) {
    let response = VersionResponse {
        version: env!("CARGO_PKG_VERSION").to_string(),
    };

    (StatusCode::OK, Json(response))
}
