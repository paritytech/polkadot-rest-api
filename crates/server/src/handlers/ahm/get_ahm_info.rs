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

#[utoipa::path(
    get,
    path = "/v1/ahm-info",
    tag = "ahm",
    summary = "Asset Hub Migration info",
    description = "Returns information about the Asset Hub migration, including start and end blocks for both relay chain and Asset Hub.",
    responses(
        (status = 200, description = "AHM migration boundaries", body = Object),
        (status = 404, description = "No migration data available")
    )
)]
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
    use crate::state::{AppState, ChainInfo};
    use config::SidecarConfig;
    use serde_json::json;
    use std::sync::Arc;
    use subxt_rpcs::client::mock_rpc_client::Json;
    use subxt_rpcs::client::{MockRpcClient, RpcClient};

    /// Helper to create a test AppState with mocked RPC client and custom chain info
    async fn create_test_state_with_chain_info(chain_type: ChainType, spec_name: &str) -> AppState {
        let config = SidecarConfig::default();
        let mock_client = MockRpcClient::builder()
            .method_handler("rpc_methods", async |_params| {
                Json(json!({ "methods": [] }))
            })
            .method_handler("chain_getBlockHash", async |_params| {
                Json("0x0000000000000000000000000000000000000000000000000000000000000000")
            })
            .build();
        let rpc_client = Arc::new(RpcClient::new(mock_client));
        let legacy_rpc = Arc::new(subxt_rpcs::LegacyRpcMethods::new((*rpc_client).clone()));
        let chain_info = ChainInfo {
            chain_type,
            spec_name: spec_name.to_string(),
            spec_version: 1,
            ss58_prefix: 42,
        };

        let client = subxt::OnlineClient::from_rpc_client((*rpc_client).clone())
            .await
            .expect("Failed to create test OnlineClient");

        AppState {
            config,
            client: Arc::new(client),
            legacy_rpc,
            rpc_client,
            chain_info,
            relay_client: None,
            relay_rpc_client: None,
            relay_chain_info: None,
            fee_details_cache: Arc::new(crate::utils::QueryFeeDetailsCache::new()),
            chain_configs: Arc::new(config::ChainConfigs::default()),
            chain_config: Arc::new(config::Config::single_chain(config::ChainConfig::default())),
            route_registry: crate::routes::RouteRegistry::new(),
            relay_chain_rpc: None,
            lazy_relay_rpc: Arc::new(tokio::sync::OnceCell::new()),
        }
    }

    #[tokio::test]
    async fn test_ahm_info_asset_hub_westmint() {
        // Test: Asset Hub with static boundaries (westmint)
        let state = create_test_state_with_chain_info(ChainType::AssetHub, "westmint").await;

        let result = ahm_info(State(state)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        assert_eq!(response.relay.start_block, Some(26041702));
        assert_eq!(response.relay.end_block, Some(26071771));
        assert_eq!(response.asset_hub.start_block, Some(11716733));
        assert_eq!(response.asset_hub.end_block, Some(11736597));
    }

    #[tokio::test]
    async fn test_ahm_info_asset_hub_statemint() {
        // Test: Asset Hub Polkadot (statemint)
        let state = create_test_state_with_chain_info(ChainType::AssetHub, "statemint").await;

        let result = ahm_info(State(state)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        assert_eq!(response.relay.start_block, Some(28490502));
        assert_eq!(response.relay.end_block, Some(28495696));
        assert_eq!(response.asset_hub.start_block, Some(10254470));
        assert_eq!(response.asset_hub.end_block, Some(10259208));
    }

    #[tokio::test]
    async fn test_ahm_info_asset_hub_statemine() {
        // Test: Asset Hub Kusama (statemine)
        let state = create_test_state_with_chain_info(ChainType::AssetHub, "statemine").await;

        let result = ahm_info(State(state)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        assert_eq!(response.relay.start_block, Some(30423691));
        assert_eq!(response.relay.end_block, Some(30425590));
        assert_eq!(response.asset_hub.start_block, Some(11150168));
        assert_eq!(response.asset_hub.end_block, Some(11151931));
    }

    #[tokio::test]
    async fn test_ahm_info_relay_westend() {
        // Test: Relay Chain (Westend) maps to westmint
        let state = create_test_state_with_chain_info(ChainType::Relay, "westend").await;

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
        // Test: Relay Chain (Polkadot) maps to statemint
        let state = create_test_state_with_chain_info(ChainType::Relay, "polkadot").await;

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
        // Test: Relay Chain (Kusama) maps to statemine
        let state = create_test_state_with_chain_info(ChainType::Relay, "kusama").await;

        let result = ahm_info(State(state)).await;

        assert!(result.is_ok());
        let response = result.unwrap().0;

        assert_eq!(response.relay.start_block, Some(30423691));
        assert_eq!(response.relay.end_block, Some(30425590));
        assert_eq!(response.asset_hub.start_block, Some(11150168));
        assert_eq!(response.asset_hub.end_block, Some(11151931));
    }

    #[tokio::test]
    async fn test_ahm_info_invalid_chain_type() {
        // Test: Parachain type should return error
        let state = create_test_state_with_chain_info(ChainType::Parachain, "some-parachain").await;

        let result = ahm_info(State(state)).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            GetAhmInfoError::NoMigrationData(name) => {
                assert_eq!(name, "some-parachain");
            }
            _ => panic!("Expected NoMigrationData error"),
        }
    }

    #[tokio::test]
    async fn test_ahm_info_unknown_relay() {
        // Test: Unknown relay chain should return error
        let state = create_test_state_with_chain_info(ChainType::Relay, "unknown-relay").await;

        let result = ahm_info(State(state)).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            GetAhmInfoError::InvalidChainSpec => {}
            _ => panic!("Expected InvalidChainSpec error"),
        }
    }

    #[tokio::test]
    async fn test_ahm_info_unknown_asset_hub() {
        // Test: Unknown asset hub should return error
        let state =
            create_test_state_with_chain_info(ChainType::AssetHub, "unknown-asset-hub").await;

        let result = ahm_info(State(state)).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            GetAhmInfoError::NoMigrationData(name) => {
                assert_eq!(name, "unknown-asset-hub");
            }
            _ => panic!("Expected NoMigrationData error"),
        }
    }
}
