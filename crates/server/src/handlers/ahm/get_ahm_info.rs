use crate::consts::{get_asset_hub_spec_name, get_migration_boundaries};
use crate::state::AppState;
use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use config::ChainType;
use serde::Serialize;
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GetAhmInfoError {
    #[error("Invalid chain specName. Can't map specName to asset hub spec")]
    InvalidChainSpec,

    #[error("No migration data available for chain: {0}")]
    NoMigrationData(String),
}

impl IntoResponse for GetAhmInfoError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match self {
            GetAhmInfoError::InvalidChainSpec | GetAhmInfoError::NoMigrationData(_) => {
                (StatusCode::NOT_FOUND, self.to_string())
            }
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AhmStartEndBlocks {
    #[serde(serialize_with = "serialize_option_u32_as_string")]
    pub start_block: Option<u32>,
    #[serde(serialize_with = "serialize_option_u32_as_string")]
    pub end_block: Option<u32>,
}

/// Serialize Option<u32> as Option<String> to match sidecar's behavior
fn serialize_option_u32_as_string<S>(value: &Option<u32>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match value {
        Some(v) => serializer.serialize_some(&v.to_string()),
        None => serializer.serialize_none(),
    }
}

#[derive(Debug, Serialize)]
pub struct AhmInfoResponse {
    pub relay: AhmStartEndBlocks,
    #[serde(rename = "assetHub")]
    pub asset_hub: AhmStartEndBlocks,
}

/// Get Asset Hub Migration information
///
/// This endpoint returns information about the Asset Hub migration, including
/// start and end blocks for both relay chain and Asset Hub.
///
/// Returns:
/// - Information about migration boundaries for relay and asset hub
pub async fn ahm_info(
    State(state): State<AppState>,
) -> Result<Json<AhmInfoResponse>, GetAhmInfoError> {
    // Determine if we're connected to a relay chain or asset hub
    let (relay, asset_hub) = match state.chain_info.chain_type {
        ChainType::AssetHub => handle_from_asset_hub(&state)?,
        ChainType::Relay => handle_from_relay(&state)?,
        _ => {
            return Err(GetAhmInfoError::NoMigrationData(
                state.chain_info.spec_name.clone(),
            ));
        }
    };

    Ok(Json(AhmInfoResponse { relay, asset_hub }))
}

/// Handle AHM info when connected to Asset Hub
fn handle_from_asset_hub(
    state: &AppState,
) -> Result<(AhmStartEndBlocks, AhmStartEndBlocks), GetAhmInfoError> {
    let spec_name = &state.chain_info.spec_name;

    let boundaries = get_migration_boundaries(spec_name.as_str())
        .ok_or_else(|| GetAhmInfoError::NoMigrationData(spec_name.clone()))?;

    Ok((
        AhmStartEndBlocks {
            start_block: Some(boundaries.relay_migration_started_at),
            end_block: Some(boundaries.relay_migration_ended_at),
        },
        AhmStartEndBlocks {
            start_block: Some(boundaries.asset_hub_migration_started_at),
            end_block: Some(boundaries.asset_hub_migration_ended_at),
        },
    ))
}

/// Handle AHM info when connected to Relay Chain
fn handle_from_relay(
    state: &AppState,
) -> Result<(AhmStartEndBlocks, AhmStartEndBlocks), GetAhmInfoError> {
    let spec_name = &state.chain_info.spec_name;

    // Map relay spec name to asset hub spec name
    let asset_hub_spec_name =
        get_asset_hub_spec_name(spec_name.as_str()).ok_or(GetAhmInfoError::InvalidChainSpec)?;

    let boundaries = get_migration_boundaries(asset_hub_spec_name)
        .ok_or_else(|| GetAhmInfoError::NoMigrationData(spec_name.clone()))?;

    Ok((
        AhmStartEndBlocks {
            start_block: Some(boundaries.relay_migration_started_at),
            end_block: Some(boundaries.relay_migration_ended_at),
        },
        AhmStartEndBlocks {
            start_block: Some(boundaries.asset_hub_migration_started_at),
            end_block: Some(boundaries.asset_hub_migration_ended_at),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use config::SidecarConfig;

    async fn create_state_with_url(url: &str) -> AppState {
        let mut config = SidecarConfig::default();
        config.substrate.url = url.to_string();
        AppState::new_with_config(config)
            .await
            .expect("Failed to create AppState")
    }

    #[tokio::test]
    async fn test_ahm_info_asset_hub_westmint() {
        // Test: Asset Hub with static boundaries (westmint)
        // This should return static migration boundaries
        let state = create_state_with_url("wss://westmint-rpc.polkadot.io").await;

        // Verify we're connected to westmint
        assert_eq!(state.chain_info.spec_name, "westmint");

        let result = ahm_info(State(state)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        assert_eq!(response.relay.start_block, Some(26041702));
        assert_eq!(response.relay.end_block, Some(26071771));
        assert_eq!(response.asset_hub.start_block, Some(11716733));
        assert_eq!(response.asset_hub.end_block, Some(11736597));
    }

    #[tokio::test]
    async fn test_ahm_info_relay_westend() {
        // Test: Relay Chain with static boundaries (westend)
        // This should map westend -> westmint and return static boundaries
        let state = create_state_with_url("wss://westend-rpc.polkadot.io").await;

        // Verify we're connected to westend
        assert_eq!(state.chain_info.spec_name, "westend");

        let result = ahm_info(State(state)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        assert_eq!(response.relay.start_block, Some(26041702));
        assert_eq!(response.relay.end_block, Some(26071771));
        assert_eq!(response.asset_hub.start_block, Some(11716733));
        assert_eq!(response.asset_hub.end_block, Some(11736597));
    }

    #[tokio::test]
    async fn test_ahm_info_relay_polkadot() {
        // Test: Relay Chain (Polkadot) with static boundaries
        // This should map polkadot -> statemint and return static boundaries
        let state = create_state_with_url("wss://rpc.polkadot.io").await;

        // Verify we're connected to polkadot
        assert_eq!(state.chain_info.spec_name, "polkadot");

        let result = ahm_info(State(state)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        assert_eq!(response.relay.start_block, Some(28490502));
        assert_eq!(response.relay.end_block, Some(28495696));
        assert_eq!(response.asset_hub.start_block, Some(10254470));
        assert_eq!(response.asset_hub.end_block, Some(10259208));
    }

    #[tokio::test]
    async fn test_ahm_info_relay_kusama() {
        // Test: Relay Chain (Kusama) with static boundaries
        // This should map kusama -> statemine and return static boundaries
        let state = create_state_with_url("wss://kusama-rpc.polkadot.io").await;

        // Verify we're connected to kusama
        assert_eq!(state.chain_info.spec_name, "kusama");

        let result = ahm_info(State(state)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        assert_eq!(response.relay.start_block, Some(30423691));
        assert_eq!(response.relay.end_block, Some(30425590));
        assert_eq!(response.asset_hub.start_block, Some(11150168));
        assert_eq!(response.asset_hub.end_block, Some(11151931));
    }
}
