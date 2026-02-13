// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::handlers::node::common::{
    FetchError, TipExtractionError, fetch_transaction_pool_simple, fetch_transaction_pool_with_fees,
};

// Re-export for tests
#[cfg(test)]
use crate::handlers::node::common::extract_tip_from_extrinsic_bytes;
use crate::handlers::node::{TransactionPoolQueryParams, TransactionPoolResponse};
use crate::state::{AppState, RelayChainError};
use crate::utils;
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GetRcNodeTransactionPoolError {
    #[error(transparent)]
    RelayChain(#[from] RelayChainError),

    #[error("Failed to get pending extrinsics")]
    PendingExtrinsicsFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get fee info")]
    FeeInfoFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get fee details")]
    FeeDetailsFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get block hash")]
    BlockHashFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get metadata")]
    MetadataFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to decode metadata")]
    MetadataDecodeFailed(#[source] parity_scale_codec::Error),

    #[error("Constant not found: {0}")]
    ConstantNotFound(String),

    #[error("Tip extraction failed: {0}")]
    TipExtractionFailed(#[from] TipExtractionError),
}

impl From<FetchError> for GetRcNodeTransactionPoolError {
    fn from(err: FetchError) -> Self {
        match err {
            FetchError::RpcFailed(e) => GetRcNodeTransactionPoolError::PendingExtrinsicsFailed(e),
            FetchError::MetadataDecodeFailed(e) => {
                GetRcNodeTransactionPoolError::MetadataDecodeFailed(e)
            }
            FetchError::ConstantNotFound(s) => GetRcNodeTransactionPoolError::ConstantNotFound(s),
            FetchError::TipExtraction(e) => GetRcNodeTransactionPoolError::TipExtractionFailed(e),
        }
    }
}

impl IntoResponse for GetRcNodeTransactionPoolError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            GetRcNodeTransactionPoolError::RelayChain(RelayChainError::NotConfigured) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            GetRcNodeTransactionPoolError::RelayChain(RelayChainError::ConnectionFailed(_)) => {
                (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
            }
            GetRcNodeTransactionPoolError::PendingExtrinsicsFailed(err)
            | GetRcNodeTransactionPoolError::FeeInfoFailed(err)
            | GetRcNodeTransactionPoolError::FeeDetailsFailed(err)
            | GetRcNodeTransactionPoolError::BlockHashFailed(err)
            | GetRcNodeTransactionPoolError::MetadataFailed(err) => utils::rpc_error_to_status(err),
            GetRcNodeTransactionPoolError::MetadataDecodeFailed(_)
            | GetRcNodeTransactionPoolError::ConstantNotFound(_)
            | GetRcNodeTransactionPoolError::TipExtractionFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

/// Handler for GET /rc/node/transaction-pool
///
/// Returns the relay chain's transaction pool with optional fee information.
/// This endpoint is specifically for Asset Hub instances to query relay chain
/// pending transactions.
#[utoipa::path(
    get,
    path = "/v1/rc/node/transaction-pool",
    tag = "rc",
    summary = "RC get transaction pool",
    description = "Returns the relay chain's transaction pool with optional fee information.",
    params(
        ("includeFee" = Option<bool>, Query, description = "Include fee information for each transaction (default: false)")
    ),
    responses(
        (status = 200, description = "Relay chain transaction pool", body = TransactionPoolResponse),
        (status = 503, description = "Relay chain not configured"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_rc_node_transaction_pool(
    State(state): State<AppState>,
    Query(params): Query<TransactionPoolQueryParams>,
) -> Result<Json<TransactionPoolResponse>, GetRcNodeTransactionPoolError> {
    let relay_rpc_client = state.get_or_init_relay_rpc_client().await?;

    let response = if params.include_fee {
        fetch_transaction_pool_with_fees(&relay_rpc_client).await?
    } else {
        fetch_transaction_pool_simple(&relay_rpc_client).await?
    };

    Ok(Json(response))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use crate::test_fixtures::mock_rpc_client_builder;
    use axum::extract::{Query, State};
    use config::SidecarConfig;
    use std::sync::Arc;
    use subxt_rpcs::client::mock_rpc_client::Json as MockJson;
    use subxt_rpcs::client::{MockRpcClient, RpcClient};

    async fn create_test_state_with_relay_mock(relay_mock_client: MockRpcClient) -> AppState {
        let config = SidecarConfig::default();
        let primary_mock = mock_rpc_client_builder().build();
        let rpc_client = Arc::new(RpcClient::new(primary_mock));
        let relay_rpc_client = Arc::new(RpcClient::new(relay_mock_client));
        let legacy_rpc = Arc::new(subxt_rpcs::LegacyRpcMethods::new((*rpc_client).clone()));
        let chain_info = crate::state::ChainInfo {
            chain_type: config::ChainType::AssetHub,
            spec_name: "statemint".to_string(),
            spec_version: 1,
            ss58_prefix: 0,
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
            relay_rpc_client: Some(relay_rpc_client.clone()),
            relay_chain_rpc: Some(Arc::new(subxt_rpcs::LegacyRpcMethods::new(
                (*relay_rpc_client).clone(),
            ))),
            relay_chain_info: None,
            fee_details_cache: Arc::new(crate::utils::QueryFeeDetailsCache::new()),
            chain_configs: Arc::new(config::ChainConfigs::default()),
            chain_config: Arc::new(config::Config::single_chain(config::ChainConfig::default())),
            route_registry: crate::routes::RouteRegistry::new(),
            lazy_relay_rpc: Arc::new(tokio::sync::OnceCell::new()),
        }
    }

    fn real_polkadot_extrinsic_tip_zero() -> String {
        "0xed098400af3e1db41e95040f7630e64d1b3104235c08545e452b15fd70601881aa224b740048ceb5c1995db4427ba1322f48702cebe4b4564e03d660d6a713f25e48143be454875d56716def88a61283643fcb9a0aed7caccbfe285dfba8399b07bc448c063501740001070540000000966d74f8027e07b43717b6876d97544fe0d71facef06acc8382749ae944e00005fa73637062b".to_string()
    }

    fn build_extrinsic_with_tip(tip: u128) -> String {
        use parity_scale_codec::{Compact, Encode};

        let mut body = vec![0x84];
        body.push(0x00);
        body.extend_from_slice(&[0x42u8; 32]);
        body.push(0x01);
        body.extend_from_slice(&[0xAAu8; 64]);
        body.push(0x00);
        Compact(1u32).encode_to(&mut body);
        Compact(tip).encode_to(&mut body);
        body.push(0x00);
        body.push(0x00);

        let mut extrinsic = Vec::new();
        Compact(body.len() as u32).encode_to(&mut extrinsic);
        extrinsic.extend(body);

        format!("0x{}", hex::encode(&extrinsic))
    }

    #[tokio::test]
    async fn test_rc_transaction_pool_empty() {
        let relay_mock = mock_rpc_client_builder()
            .method_handler("author_pendingExtrinsics", async |_params| {
                MockJson(serde_json::json!([]))
            })
            .build();

        let state = create_test_state_with_relay_mock(relay_mock).await;
        let params = TransactionPoolQueryParams { include_fee: false };

        let result = get_rc_node_transaction_pool(State(state), Query(params)).await;
        assert!(result.is_ok());

        let response = result.unwrap().0;
        assert_eq!(response.pool.len(), 0);
    }

    #[tokio::test]
    async fn test_rc_transaction_pool_without_fee() {
        let extrinsic = real_polkadot_extrinsic_tip_zero();

        let relay_mock = mock_rpc_client_builder()
            .method_handler("author_pendingExtrinsics", async |_params| {
                MockJson(serde_json::json!([real_polkadot_extrinsic_tip_zero()]))
            })
            .build();

        let state = create_test_state_with_relay_mock(relay_mock).await;
        let params = TransactionPoolQueryParams { include_fee: false };

        let result = get_rc_node_transaction_pool(State(state), Query(params)).await;
        assert!(result.is_ok());

        let response = result.unwrap().0;
        assert_eq!(response.pool.len(), 1);

        let entry = &response.pool[0];
        assert!(!entry.hash.is_empty());
        assert_eq!(entry.encoded_extrinsic, extrinsic);
        assert!(entry.tip.is_none());
        assert!(entry.priority.is_none());
        assert!(entry.partial_fee.is_none());
    }

    #[tokio::test]
    async fn test_rc_transaction_pool_with_fee() {
        let extrinsic = real_polkadot_extrinsic_tip_zero();

        let relay_mock = mock_rpc_client_builder()
            .method_handler("author_pendingExtrinsics", async |_params| {
                MockJson(serde_json::json!([real_polkadot_extrinsic_tip_zero()]))
            })
            .method_handler("chain_getFinalizedHead", async |_params| {
                MockJson("0x1234567890123456789012345678901234567890123456789012345678901234")
            })
            .method_handler("payment_queryInfo", async |_params| {
                MockJson(serde_json::json!({
                    "weight": { "refTime": "1000000", "proofSize": "0" },
                    "class": "Normal",
                    "partialFee": "10000000"
                }))
            })
            .method_handler("state_getMetadata", async |_params| MockJson("0x6d657461"))
            .build();

        let state = create_test_state_with_relay_mock(relay_mock).await;
        let params = TransactionPoolQueryParams { include_fee: true };

        let result = get_rc_node_transaction_pool(State(state), Query(params)).await;
        if let Ok(response) = result {
            assert_eq!(response.pool.len(), 1);
            let entry = &response.pool[0];
            assert!(!entry.hash.is_empty());
            assert_eq!(entry.encoded_extrinsic, extrinsic);
            assert_eq!(entry.tip, Some("0".to_string()));
            assert_eq!(entry.partial_fee, Some("10000000".to_string()));
        }
    }

    #[test]
    fn test_extract_tip_real_polkadot_extrinsic() {
        let extrinsic_hex = real_polkadot_extrinsic_tip_zero();
        let extrinsic_bytes = hex::decode(extrinsic_hex.trim_start_matches("0x")).unwrap();

        let tip = extract_tip_from_extrinsic_bytes(&extrinsic_bytes).expect("should parse");
        assert_eq!(tip, Some("0".to_string()));
    }

    #[test]
    fn test_extract_tip_synthetic_values() {
        for expected_tip in [1u128, 100, 1000, 1_000_000, u64::MAX as u128] {
            let extrinsic_hex = build_extrinsic_with_tip(expected_tip);
            let extrinsic_bytes = hex::decode(extrinsic_hex.trim_start_matches("0x")).unwrap();

            let tip = extract_tip_from_extrinsic_bytes(&extrinsic_bytes).expect("should parse");
            assert_eq!(
                tip,
                Some(expected_tip.to_string()),
                "Failed for tip: {}",
                expected_tip
            );
        }
    }

    #[test]
    fn test_extract_tip_edge_cases() {
        use parity_scale_codec::{Compact, Encode};

        // Empty input should return an error
        assert!(extract_tip_from_extrinsic_bytes(&[]).is_err());

        // Single byte is malformed
        assert!(extract_tip_from_extrinsic_bytes(&[0x00]).is_err());

        // Unsigned extrinsic should return Ok(None)
        let body = vec![0x04, 0x00, 0x00];
        let mut unsigned = Vec::new();
        Compact(body.len() as u32).encode_to(&mut unsigned);
        unsigned.extend(body);
        assert_eq!(extract_tip_from_extrinsic_bytes(&unsigned), Ok(None));
    }
}
