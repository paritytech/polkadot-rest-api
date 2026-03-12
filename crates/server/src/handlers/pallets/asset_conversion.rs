// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Handlers for /pallets/asset-conversion endpoints.
//!
//! This module provides endpoints for querying the AssetConversion pallet:
//! - `/pallets/asset-conversion/liquidity-pools` - List all liquidity pools
//! - `/pallets/asset-conversion/next-available-id` - Get the next available pool asset ID

use crate::extractors::JsonQuery;
use crate::handlers::pallets::common::{AtResponse, PalletError, resolve_block_for_pallet};
use crate::handlers::runtime_queries::asset_conversion as asset_conversion_queries;
use crate::handlers::runtime_queries::staking as staking_queries;
use crate::state::AppState;
use crate::utils;
use crate::utils::rc_block::find_ah_blocks_in_rc_block;
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use heck::ToLowerCamelCase;
use polkadot_rest_api_config::ChainType;
use scale_decode::DecodeAsType;
use serde::{Deserialize, Serialize};
use subxt::{SubstrateConfig, client::OnlineClientAtBlock};

// ============================================================================
// Request/Response Types
// ============================================================================

// --- SCALE Decode Types for AssetConversion::Pools ---

/// Pool info value (contains LP token ID)
#[derive(Debug, DecodeAsType, Serialize)]
#[serde(rename_all = "camelCase")]
struct PoolInfo {
    lp_token: u32,
}

/// Converts a `scale_value::Value` to `serde_json::Value`, matching Sidecar's JSON format.
///
/// The pool key type varies across chains (older chains used `NativeOrWithId`,
/// newer ones use XCM `Location`), so we decode the key dynamically via
/// `scale_value::Value<()>` and convert to JSON here.
fn scale_value_to_json(value: &scale_value::Value) -> serde_json::Value {
    match &value.value {
        scale_value::ValueDef::Composite(composite) => match composite {
            scale_value::Composite::Named(fields) => {
                let map: serde_json::Map<String, serde_json::Value> = fields
                    .iter()
                    .map(|(name, val)| (name.to_lower_camel_case(), scale_value_to_json(val)))
                    .collect();
                serde_json::Value::Object(map)
            }
            scale_value::Composite::Unnamed(values) => {
                if values.len() == 1 {
                    scale_value_to_json(&values[0])
                } else {
                    serde_json::Value::Array(values.iter().map(scale_value_to_json).collect())
                }
            }
        },
        scale_value::ValueDef::Variant(variant) => {
            let variant_value = scale_value_to_json(&scale_value::Value {
                value: scale_value::ValueDef::Composite(variant.values.clone()),
                context: (),
            });

            let variant_name = variant.name.to_lower_camel_case();
            if variant_value.is_null()
                || (variant_value.is_array()
                    && variant_value.as_array().is_some_and(|a| a.is_empty()))
            {
                serde_json::json!({ variant_name: null })
            } else {
                serde_json::json!({ variant_name: variant_value })
            }
        }
        scale_value::ValueDef::Primitive(primitive) => match primitive {
            scale_value::Primitive::Bool(b) => serde_json::Value::Bool(*b),
            scale_value::Primitive::Char(c) => serde_json::Value::String(c.to_string()),
            scale_value::Primitive::String(s) => serde_json::Value::String(s.clone()),
            scale_value::Primitive::U128(n) => serde_json::Value::String(n.to_string()),
            scale_value::Primitive::I128(n) => serde_json::Value::String(n.to_string()),
            scale_value::Primitive::U256(n) => serde_json::Value::String(format!("{:?}", n)),
            scale_value::Primitive::I256(n) => serde_json::Value::String(format!("{:?}", n)),
        },
        scale_value::ValueDef::BitSequence(bits) => {
            serde_json::Value::String(format!("{:?}", bits))
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetConversionQueryParams {
    pub at: Option<String>,
    #[serde(default)]
    pub use_rc_block: bool,
}

// --- Next Available ID Response ---

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NextAvailableIdResponse {
    pub at: AtResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}

// --- Liquidity Pools Response ---

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiquidityPoolInfo {
    /// The pair of assets in the pool, represented as JSON
    pub reserves: serde_json::Value,
    /// The LP token info for this pool
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lp_token: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiquidityPoolsResponse {
    pub at: AtResponse,
    pub pools: Vec<LiquidityPoolInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}

// ============================================================================
// Next Available ID Handler
// ============================================================================

#[utoipa::path(
    get,
    path = "/v1/pallets/asset-conversion/next-available-id",
    tag = "pallets",
    summary = "Next available pool ID",
    description = "Returns the next available pool asset ID from the AssetConversion pallet.",
    params(
        ("at" = Option<String>, Query, description = "Block hash or number to query at"),
        ("useRcBlock" = Option<bool>, Query, description = "Treat 'at' as relay chain block identifier")
    ),
    responses(
        (status = 200, description = "Next available ID", body = Object),
        (status = 503, description = "Service unavailable"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_next_available_id(
    State(state): State<AppState>,
    JsonQuery(params): JsonQuery<AssetConversionQueryParams>,
) -> Result<Response, PalletError> {
    if params.use_rc_block {
        return handle_next_id_with_rc_block(state, params).await;
    }

    // Resolve block using the common helper
    let resolved = resolve_block_for_pallet(&state.client, params.at.as_ref()).await?;

    let pool_id = fetch_next_pool_asset_id(&resolved.client_at_block).await?;

    Ok((
        StatusCode::OK,
        Json(NextAvailableIdResponse {
            at: resolved.at,
            pool_id,
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        }),
    )
        .into_response())
}

/// Handles the next-available-id request with RC block resolution.
async fn handle_next_id_with_rc_block(
    state: AppState,
    params: AssetConversionQueryParams,
) -> Result<Response, PalletError> {
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(PalletError::UseRcBlockNotSupported);
    }

    let rc_rpc_client = state.get_relay_chain_rpc_client().await?;
    let rc_rpc = state.get_relay_chain_rpc().await?;

    let rc_block_id = params
        .at
        .as_ref()
        .ok_or(PalletError::AtParameterRequired)?
        .parse::<utils::BlockId>()?;

    let rc_resolved_block =
        utils::resolve_block_with_rpc(&rc_rpc_client, &rc_rpc, Some(rc_block_id)).await?;

    let ah_blocks = find_ah_blocks_in_rc_block(&state, &rc_resolved_block).await?;

    // Return empty array when no AH blocks found (matching Sidecar behavior)
    if ah_blocks.is_empty() {
        return Ok((StatusCode::OK, Json(serde_json::json!([]))).into_response());
    }

    let rc_block_number = rc_resolved_block.number.to_string();
    let rc_block_hash = rc_resolved_block.hash.clone();

    // Process ALL AH blocks, not just the first one
    let mut results = Vec::new();
    for ah_block in ah_blocks {
        let client_at_block = state.client.at_block(ah_block.number).await?;

        let at = AtResponse {
            hash: ah_block.hash.clone(),
            height: ah_block.number.to_string(),
        };

        let ah_timestamp = fetch_timestamp(&client_at_block).await;
        let pool_id = fetch_next_pool_asset_id(&client_at_block).await?;

        results.push(NextAvailableIdResponse {
            at,
            pool_id,
            rc_block_hash: Some(rc_block_hash.clone()),
            rc_block_number: Some(rc_block_number.clone()),
            ah_timestamp,
        });
    }

    Ok((StatusCode::OK, Json(results)).into_response())
}

// ============================================================================
// Liquidity Pools Handler
// ============================================================================

#[utoipa::path(
    get,
    path = "/v1/pallets/asset-conversion/liquidity-pools",
    tag = "pallets",
    summary = "Liquidity pools",
    description = "Returns all liquidity pools from the AssetConversion pallet.",
    params(
        ("at" = Option<String>, Query, description = "Block hash or number to query at"),
        ("useRcBlock" = Option<bool>, Query, description = "Treat 'at' as relay chain block identifier")
    ),
    responses(
        (status = 200, description = "Liquidity pools", body = Object),
        (status = 503, description = "Service unavailable"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_liquidity_pools(
    State(state): State<AppState>,
    JsonQuery(params): JsonQuery<AssetConversionQueryParams>,
) -> Result<Response, PalletError> {
    if params.use_rc_block {
        return handle_pools_with_rc_block(state, params).await;
    }

    // Resolve block using the common helper
    let resolved = resolve_block_for_pallet(&state.client, params.at.as_ref()).await?;

    let pools = fetch_liquidity_pools(&resolved.client_at_block).await?;

    Ok((
        StatusCode::OK,
        Json(LiquidityPoolsResponse {
            at: resolved.at,
            pools,
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        }),
    )
        .into_response())
}

/// Handles the liquidity-pools request with RC block resolution.
async fn handle_pools_with_rc_block(
    state: AppState,
    params: AssetConversionQueryParams,
) -> Result<Response, PalletError> {
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(PalletError::UseRcBlockNotSupported);
    }

    let rc_rpc_client = state.get_relay_chain_rpc_client().await?;
    let rc_rpc = state.get_relay_chain_rpc().await?;

    let rc_block_id = params
        .at
        .as_ref()
        .ok_or(PalletError::AtParameterRequired)?
        .parse::<utils::BlockId>()?;

    let rc_resolved_block =
        utils::resolve_block_with_rpc(&rc_rpc_client, &rc_rpc, Some(rc_block_id)).await?;

    let ah_blocks = find_ah_blocks_in_rc_block(&state, &rc_resolved_block).await?;

    // Return empty array when no AH blocks found (matching Sidecar behavior)
    if ah_blocks.is_empty() {
        return Ok((StatusCode::OK, Json(serde_json::json!([]))).into_response());
    }

    let rc_block_number = rc_resolved_block.number.to_string();
    let rc_block_hash = rc_resolved_block.hash.clone();

    // Process ALL AH blocks, not just the first one
    let mut results = Vec::new();
    for ah_block in ah_blocks {
        let client_at_block = state.client.at_block(ah_block.number).await?;

        let at = AtResponse {
            hash: ah_block.hash.clone(),
            height: ah_block.number.to_string(),
        };

        let ah_timestamp = fetch_timestamp(&client_at_block).await;
        let pools = fetch_liquidity_pools(&client_at_block).await?;

        results.push(LiquidityPoolsResponse {
            at,
            pools,
            rc_block_hash: Some(rc_block_hash.clone()),
            rc_block_number: Some(rc_block_number.clone()),
            ah_timestamp,
        });
    }

    Ok((StatusCode::OK, Json(results)).into_response())
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Fetches the next available pool asset ID from AssetConversion::NextPoolAssetId storage.
/// Returns an error if the pallet doesn't exist.
async fn fetch_next_pool_asset_id(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Option<String>, PalletError> {
    // Use centralized query function
    Ok(
        asset_conversion_queries::get_next_pool_asset_id(client_at_block)
            .await
            .map(|id| id.to_string()),
    )
}

/// Fetches all liquidity pools from AssetConversion::Pools storage.
/// Returns an error if the pallet doesn't exist.
async fn fetch_liquidity_pools(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<LiquidityPoolInfo>, PalletError> {
    let mut pools = Vec::new();

    // Iterate all entries in AssetConversion::Pools storage.
    // The key type varies across chains (NativeOrWithId on older chains, XCM Location
    // on newer ones), so we use dynamic string-based address for iteration and
    // decode keys via scale_value::Value<()>.
    let storage_addr = ("AssetConversion", "Pools");
    let key_parts: Vec<scale_value::Value> = vec![];

    let mut iter = client_at_block
        .storage()
        .iter(storage_addr, key_parts)
        .await
        .map_err(|e| {
            tracing::error!("Failed to iterate AssetConversion::Pools storage: {:?}", e);
            PalletError::PalletNotFound("AssetConversion".to_string())
        })?;

    while let Some(result) = iter.next().await {
        match result {
            Ok(kv) => {
                // Decode the key dynamically using scale_value::Value<()>.
                // The key contains the asset pair (reserves) — which could be
                // (NativeOrWithId, NativeOrWithId) or (Location, Location) depending on the chain.
                let reserves = match kv.key() {
                    Ok(storage_key) => match storage_key.decode() {
                        Ok(key_values) => {
                            if key_values.len() == 1 {
                                scale_value_to_json(&key_values[0])
                            } else if key_values.is_empty() {
                                serde_json::Value::Null
                            } else {
                                serde_json::Value::Array(
                                    key_values.iter().map(scale_value_to_json).collect(),
                                )
                            }
                        }
                        Err(_) => serde_json::Value::Null,
                    },
                    Err(_) => serde_json::Value::Null,
                };

                // Decode value as typed PoolInfo (lp_token)
                let lp_token = match kv.value().decode_as::<PoolInfo>() {
                    Ok(pool_info) => {
                        Some(serde_json::json!({"lpToken": pool_info.lp_token.to_string()}))
                    }
                    Err(_) => None,
                };

                pools.push(LiquidityPoolInfo { reserves, lp_token });
            }
            Err(e) => {
                tracing::warn!("Error iterating pools: {}", e);
                continue;
            }
        }
    }

    Ok(pools)
}

/// Fetches timestamp from Timestamp::Now storage.
async fn fetch_timestamp(client_at_block: &OnlineClientAtBlock<SubstrateConfig>) -> Option<String> {
    staking_queries::get_timestamp(client_at_block)
        .await
        .map(|ts| ts.to_string())
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heck_camel_case_pascal_case() {
        assert_eq!("PascalCase".to_lower_camel_case(), "pascalCase");
        assert_eq!("Here".to_lower_camel_case(), "here");
        assert_eq!("X2".to_lower_camel_case(), "x2");
        assert_eq!("PalletInstance".to_lower_camel_case(), "palletInstance");
        assert_eq!("GeneralIndex".to_lower_camel_case(), "generalIndex");
    }

    #[test]
    fn test_scale_value_to_json_variant_empty() {
        // A variant with no values should produce {"variantName": null}
        let val = scale_value::Value {
            value: scale_value::ValueDef::Variant(scale_value::Variant {
                name: "Here".to_string(),
                values: scale_value::Composite::Unnamed(vec![]),
            }),
            context: (),
        };
        let json = scale_value_to_json(&val);
        assert_eq!(json, serde_json::json!({"here": null}));
    }

    #[test]
    fn test_scale_value_to_json_variant_with_named_fields() {
        // A variant with named fields
        let val = scale_value::Value {
            value: scale_value::ValueDef::Variant(scale_value::Variant {
                name: "PalletInstance".to_string(),
                values: scale_value::Composite::Unnamed(vec![scale_value::Value {
                    value: scale_value::ValueDef::Primitive(scale_value::Primitive::U128(50)),
                    context: (),
                }]),
            }),
            context: (),
        };
        let json = scale_value_to_json(&val);
        assert_eq!(json, serde_json::json!({"palletInstance": "50"}));
    }

    #[test]
    fn test_scale_value_to_json_named_composite() {
        let val = scale_value::Value {
            value: scale_value::ValueDef::Composite(scale_value::Composite::Named(vec![
                (
                    "parents".to_string(),
                    scale_value::Value {
                        value: scale_value::ValueDef::Primitive(scale_value::Primitive::U128(1)),
                        context: (),
                    },
                ),
                (
                    "interior".to_string(),
                    scale_value::Value {
                        value: scale_value::ValueDef::Variant(scale_value::Variant {
                            name: "Here".to_string(),
                            values: scale_value::Composite::Unnamed(vec![]),
                        }),
                        context: (),
                    },
                ),
            ])),
            context: (),
        };
        let json = scale_value_to_json(&val);
        assert_eq!(
            json,
            serde_json::json!({"parents": "1", "interior": {"here": null}})
        );
    }

    #[test]
    fn test_next_available_id_response_serialization() {
        let response = NextAvailableIdResponse {
            at: AtResponse {
                hash: "0x123".to_string(),
                height: "100".to_string(),
            },
            pool_id: Some("58".to_string()),
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        };

        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["at"]["hash"], "0x123");
        assert_eq!(json["at"]["height"], "100");
        assert_eq!(json["poolId"], "58");
        // Optional fields should not be present when None
        assert!(json.get("rcBlockHash").is_none());
        assert!(json.get("rcBlockNumber").is_none());
        assert!(json.get("ahTimestamp").is_none());
    }

    #[test]
    fn test_liquidity_pools_response_serialization() {
        let response = LiquidityPoolsResponse {
            at: AtResponse {
                hash: "0xabc".to_string(),
                height: "200".to_string(),
            },
            pools: vec![LiquidityPoolInfo {
                reserves: serde_json::json!([{"native": null}, {"asset": "1984"}]),
                lp_token: Some(serde_json::json!({"lpToken": "30"})),
            }],
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        };

        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["at"]["hash"], "0xabc");
        assert_eq!(json["at"]["height"], "200");
        assert!(json["pools"].is_array());
        assert_eq!(json["pools"].as_array().unwrap().len(), 1);
        assert!(json["pools"][0]["reserves"].is_array());
        assert!(json["pools"][0]["lpToken"].is_object());
    }

    #[test]
    fn test_liquidity_pool_info_without_lp_token() {
        let pool = LiquidityPoolInfo {
            reserves: serde_json::json!({"test": "value"}),
            lp_token: None,
        };

        let json = serde_json::to_value(&pool).unwrap();
        assert!(json.get("lpToken").is_none());
    }

    #[test]
    fn test_query_params_deserialization() {
        // Test with all fields
        let json = r#"{"at": "12345", "useRcBlock": true}"#;
        let params: AssetConversionQueryParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.at, Some("12345".to_string()));
        assert!(params.use_rc_block);

        // Test with defaults
        let json = r#"{}"#;
        let params: AssetConversionQueryParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.at, None);
        assert!(!params.use_rc_block);

        // Test with only at
        let json = r#"{"at": "0xabc123"}"#;
        let params: AssetConversionQueryParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.at, Some("0xabc123".to_string()));
        assert!(!params.use_rc_block);
    }

    #[test]
    fn test_asset_conversion_query_params_rejects_unknown_fields() {
        let json = r#"{"at": "12345", "unknownField": true}"#;
        let result: Result<AssetConversionQueryParams, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown field"));
    }
}
