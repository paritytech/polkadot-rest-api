use axum::{
    extract::State,
    response::Json,
    http::StatusCode,
};
use serde::{ Deserialize, Serialize };
use crate::state::AppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connected_chains: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime: Option<u64>
}

pub async fn get_health(
    State(_state): State<AppState>
) -> (StatusCode, Json<HealthResponse>) {
    let response = HealthResponse {
        status: "ok".to_string(),
        connected_chains: None,
        uptime: None,
    };

    (StatusCode::ACCEPTED, Json(response))
}