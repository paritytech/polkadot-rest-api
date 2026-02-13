// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::state::AppState;
use crate::utils;
use axum::{
    Json,
    extract::{Query, State},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;
use utoipa::ToSchema;

use super::common::{
    FetchError, TipExtractionError, fetch_transaction_pool_simple, fetch_transaction_pool_with_fees,
};

// Re-export for tests
#[cfg(test)]
use super::common::extract_tip_from_extrinsic_bytes;

#[derive(Debug, Error)]
pub enum GetNodeTransactionPoolError {
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

impl From<FetchError> for GetNodeTransactionPoolError {
    fn from(err: FetchError) -> Self {
        match err {
            FetchError::RpcFailed(e) => GetNodeTransactionPoolError::PendingExtrinsicsFailed(e),
            FetchError::MetadataDecodeFailed(e) => {
                GetNodeTransactionPoolError::MetadataDecodeFailed(e)
            }
            FetchError::ConstantNotFound(s) => GetNodeTransactionPoolError::ConstantNotFound(s),
            FetchError::TipExtraction(e) => GetNodeTransactionPoolError::TipExtractionFailed(e),
        }
    }
}

impl IntoResponse for GetNodeTransactionPoolError {
    fn into_response(self) -> axum::response::Response {
        use axum::http::StatusCode;

        let (status, message) = match &self {
            GetNodeTransactionPoolError::PendingExtrinsicsFailed(err)
            | GetNodeTransactionPoolError::FeeInfoFailed(err)
            | GetNodeTransactionPoolError::FeeDetailsFailed(err)
            | GetNodeTransactionPoolError::BlockHashFailed(err)
            | GetNodeTransactionPoolError::MetadataFailed(err) => utils::rpc_error_to_status(err),
            GetNodeTransactionPoolError::MetadataDecodeFailed(_)
            | GetNodeTransactionPoolError::ConstantNotFound(_)
            | GetNodeTransactionPoolError::TipExtractionFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionPoolQueryParams {
    #[serde(default)]
    pub include_fee: bool,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransactionPoolEntry {
    pub hash: String,
    pub encoded_extrinsic: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partial_fee: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransactionPoolResponse {
    pub pool: Vec<TransactionPoolEntry>,
}

#[utoipa::path(
    get,
    path = "/v1/node/transaction-pool",
    tag = "node",
    summary = "Transaction pool",
    description = "Returns the node's transaction pool with optional fee information.",
    params(
        ("includeFee" = Option<bool>, Query, description = "Include fee details for each transaction")
    ),
    responses(
        (status = 200, description = "Transaction pool entries", body = TransactionPoolResponse),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_node_transaction_pool(
    State(state): State<AppState>,
    Query(params): Query<TransactionPoolQueryParams>,
) -> Result<Json<TransactionPoolResponse>, GetNodeTransactionPoolError> {
    let response = if params.include_fee {
        fetch_transaction_pool_with_fees(&state.rpc_client).await?
    } else {
        fetch_transaction_pool_simple(&state.rpc_client).await?
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

    async fn create_test_state_with_mock(mock_client: MockRpcClient) -> AppState {
        let config = SidecarConfig::default();
        let rpc_client = Arc::new(RpcClient::new(mock_client));
        let legacy_rpc = Arc::new(subxt_rpcs::LegacyRpcMethods::new((*rpc_client).clone()));
        let chain_info = crate::state::ChainInfo {
            chain_type: config::ChainType::Relay,
            spec_name: "test".to_string(),
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

    fn create_minimal_test_metadata() -> String {
        "0x6d657461".to_string()
    }

    fn real_polkadot_extrinsic_tip_zero() -> String {
        "0xed098400af3e1db41e95040f7630e64d1b3104235c08545e452b15fd70601881aa224b740048ceb5c1995db4427ba1322f48702cebe4b4564e03d660d6a713f25e48143be454875d56716def88a61283643fcb9a0aed7caccbfe285dfba8399b07bc448c063501740001070540000000966d74f8027e07b43717b6876d97544fe0d71facef06acc8382749ae944e00005fa73637062b".to_string()
    }

    fn real_asset_hub_extrinsic_transfer() -> String {
        "0x4902840004316d995f0adb06d918a1fc96077ebdfa93aab9ccf2a8525efd7bf0c1e2282700a24152685f52e4726466e80247d965bb3d349637fc8a1ea6f7cc1451ddec98b5bf30b6e8e31b31f0870ac46f07ccb559402a0fafe90b74127f28e8644281730c00d12b0000000a0000d61e33684a7a41d7233e89955316dbc875fef1428e4f16ec260617dc57de3972078064288004".to_string()
    }

    fn real_asset_hub_extrinsic_assets() -> String {
        "0x550284000a6679243e822e0538039d187529d67c1bb74d8d5f121be63d00243233b4b01c01b8af8c2b3b7f1d020f42fc98e3957ae79957173a6e29d25fc6ad976d851ad743f4316e9e370a3a3aa3f8747f870235c18c5d8fdb75e34a831f0d0b85a9f72181f400f50b0000013208011f00d426c7726e426586d570e2ef43f3b0784fea005e80fa6e3bca9139a38f5ff1ad078068c84b0d".to_string()
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
    async fn test_transaction_pool_empty() {
        let mock_client = mock_rpc_client_builder()
            .method_handler("author_pendingExtrinsics", async |_params| {
                MockJson(serde_json::json!([]))
            })
            .build();

        let state = create_test_state_with_mock(mock_client).await;
        let params = TransactionPoolQueryParams { include_fee: false };

        let result = get_node_transaction_pool(State(state), Query(params)).await;
        assert!(result.is_ok());

        let response = result.unwrap().0;
        assert_eq!(response.pool.len(), 0);
    }

    #[tokio::test]
    async fn test_transaction_pool_without_fee_real_extrinsics() {
        let extrinsic1 = real_asset_hub_extrinsic_transfer();
        let extrinsic2 = real_asset_hub_extrinsic_assets();

        let mock_client = mock_rpc_client_builder()
            .method_handler("author_pendingExtrinsics", async |_params| {
                MockJson(serde_json::json!([
                    real_asset_hub_extrinsic_transfer(),
                    real_asset_hub_extrinsic_assets()
                ]))
            })
            .build();

        let state = create_test_state_with_mock(mock_client).await;
        let params = TransactionPoolQueryParams { include_fee: false };

        let result = get_node_transaction_pool(State(state), Query(params)).await;
        assert!(result.is_ok());

        let response = result.unwrap().0;
        assert_eq!(response.pool.len(), 2);

        let entry1 = &response.pool[0];
        assert!(!entry1.hash.is_empty());
        assert_eq!(entry1.encoded_extrinsic, extrinsic1);
        assert!(
            entry1.tip.is_none(),
            "tip should be None when includeFee=false"
        );
        assert!(entry1.priority.is_none());
        assert!(entry1.partial_fee.is_none());

        let entry2 = &response.pool[1];
        assert!(!entry2.hash.is_empty());
        assert_eq!(entry2.encoded_extrinsic, extrinsic2);
    }

    #[tokio::test]
    async fn test_transaction_pool_with_fee_real_extrinsic() {
        let extrinsic_hex = real_asset_hub_extrinsic_transfer();

        let mock_client = mock_rpc_client_builder()
            .method_handler("author_pendingExtrinsics", async |_params| {
                MockJson(serde_json::json!([real_asset_hub_extrinsic_transfer()]))
            })
            .method_handler("chain_getFinalizedHead", async |_params| {
                MockJson("0x1234567890123456789012345678901234567890123456789012345678901234")
            })
            .method_handler("payment_queryInfo", async |_params| {
                MockJson(serde_json::json!({
                    "weight": { "refTime": "1000000", "proofSize": "0" },
                    "class": "Normal",
                    "partialFee": "14668864"
                }))
            })
            .method_handler("state_getMetadata", async |_params| {
                MockJson(create_minimal_test_metadata())
            })
            .build();

        let state = create_test_state_with_mock(mock_client).await;
        let params = TransactionPoolQueryParams { include_fee: true };

        let result = get_node_transaction_pool(State(state), Query(params)).await;
        if let Ok(response) = result {
            assert_eq!(response.pool.len(), 1);
            let entry = &response.pool[0];
            assert!(!entry.hash.is_empty());
            assert_eq!(entry.encoded_extrinsic, extrinsic_hex);
            assert_eq!(entry.tip, Some("0".to_string()), "Real extrinsic has tip=0");
            assert_eq!(entry.partial_fee, Some("14668864".to_string()));
        }
    }

    #[test]
    fn test_extract_tip_real_extrinsics() {
        let test_cases = [
            ("Polkadot relay", real_polkadot_extrinsic_tip_zero(), "0"),
            ("Asset Hub", real_asset_hub_extrinsic_transfer(), "0"),
        ];

        for (name, hex, expected_tip) in test_cases {
            let bytes = hex::decode(hex.trim_start_matches("0x")).unwrap();
            let tip = extract_tip_from_extrinsic_bytes(&bytes).expect("should parse");
            assert_eq!(tip, Some(expected_tip.to_string()), "Failed for: {}", name);
        }
    }

    #[test]
    fn test_extract_tip_synthetic_various_values() {
        for expected_tip in [1u128, 100, 1000, 1_000_000, u64::MAX as u128, u128::MAX / 2] {
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
