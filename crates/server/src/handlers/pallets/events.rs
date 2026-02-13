// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Handler for the `/pallets/{palletId}/events` endpoints.
//!
//! This module provides endpoints for querying event metadata from pallets.
//! Uses Subxt's metadata API which normalizes all metadata versions internally.

#![allow(clippy::result_large_err)]

use crate::handlers::pallets::common::{
    AtResponse, PalletError, PalletItemQueryParams, PalletQueryParams, RcBlockFields,
    RcPalletItemQueryParams, RcPalletQueryParams, resolve_block_for_pallet, resolve_type_name,
};
use crate::state::AppState;
use crate::utils;
use crate::utils::rc_block::find_ah_blocks_in_rc_block;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use config::ChainType;
use heck::ToLowerCamelCase;
use serde::Serialize;
use subxt::Metadata;

// ============================================================================
// Response Types
// ============================================================================

/// Response for `/pallets/{palletId}/events`
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PalletEventsResponse {
    pub at: AtResponse,
    pub pallet: String,
    pub pallet_index: String,
    pub items: EventsItems,
    /// Relay chain block hash (Asset Hub only, when `useRcBlock=true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,
    /// Relay chain block number (Asset Hub only, when `useRcBlock=true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,
    /// Asset Hub timestamp (Asset Hub only, when `useRcBlock=true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}

/// Events items - either full metadata or just names.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum EventsItems {
    Full(Vec<EventItemMetadata>),
    OnlyIds(Vec<String>),
}

/// Response for `/pallets/{palletId}/events/{eventItemId}`
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PalletEventItemResponse {
    pub at: AtResponse,
    pub pallet: String,
    pub pallet_index: String,
    pub event_item: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<EventItemMetadata>,
    /// Relay chain block hash (Asset Hub only, when `useRcBlock=true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,
    /// Relay chain block number (Asset Hub only, when `useRcBlock=true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,
    /// Asset Hub timestamp (Asset Hub only, when `useRcBlock=true`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}

/// Metadata for a single event.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventItemMetadata {
    pub name: String,
    pub fields: Vec<EventField>,
    pub index: String,
    pub docs: Vec<String>,
    pub args: Vec<String>,
}

/// A field/argument of an event.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventField {
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,
    pub docs: Vec<String>,
}

// ============================================================================
// Main Handlers
// ============================================================================

/// Handler for GET `/pallets/{palletId}/events`
///
/// Returns all events defined in a pallet.
#[utoipa::path(
    get,
    path = "/v1/pallets/{palletId}/events",
    tag = "pallets",
    summary = "Get pallet events",
    description = "Returns all events defined in a pallet.",
    params(
        ("palletId" = String, Path, description = "Pallet name or index"),
        ("at" = Option<String>, Query, description = "Block identifier (number or hash)"),
        ("onlyIds" = Option<bool>, Query, description = "Only return event names")
    ),
    responses(
        (status = 200, description = "Pallet events", body = Object),
        (status = 400, description = "Invalid request"),
        (status = 404, description = "Pallet not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_pallet_events(
    State(state): State<AppState>,
    Path(pallet_id): Path<String>,
    Query(params): Query<PalletQueryParams>,
) -> Result<Response, PalletError> {
    if params.use_rc_block {
        return handle_events_use_rc_block(state, pallet_id, params).await;
    }

    // Resolve block using the common helper
    let resolved = resolve_block_for_pallet(&state.client, params.at.as_ref()).await?;

    let metadata = resolved.client_at_block.metadata();

    let response = extract_events_from_metadata(
        &metadata,
        &pallet_id,
        resolved.at,
        params.only_ids,
        RcBlockFields::default(),
    )?;

    Ok((StatusCode::OK, Json(response)).into_response())
}

/// Handler for GET `/pallets/{palletId}/events/{eventItemId}`
///
/// Returns metadata for a specific event in a pallet.
#[utoipa::path(
    get,
    path = "/v1/pallets/{palletId}/events/{eventItemId}",
    tag = "pallets",
    summary = "Get pallet event item",
    description = "Returns metadata for a specific event in a pallet.",
    params(
        ("palletId" = String, Path, description = "Pallet name or index"),
        ("eventItemId" = String, Path, description = "Event name"),
        ("at" = Option<String>, Query, description = "Block identifier (number or hash)"),
        ("metadata" = Option<bool>, Query, description = "Include full event metadata")
    ),
    responses(
        (status = 200, description = "Event item details", body = Object),
        (status = 400, description = "Invalid request"),
        (status = 404, description = "Pallet or event not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_pallet_event_item(
    State(state): State<AppState>,
    Path((pallet_id, event_item_id)): Path<(String, String)>,
    Query(params): Query<PalletItemQueryParams>,
) -> Result<Response, PalletError> {
    if params.use_rc_block {
        return handle_event_item_use_rc_block(state, pallet_id, event_item_id, params).await;
    }

    // Resolve block using the common helper
    let resolved = resolve_block_for_pallet(&state.client, params.at.as_ref()).await?;

    let metadata = resolved.client_at_block.metadata();

    let response = extract_event_item_from_metadata(
        &metadata,
        &pallet_id,
        &event_item_id,
        resolved.at,
        params.metadata,
        RcBlockFields::default(),
    )?;

    Ok((StatusCode::OK, Json(response)).into_response())
}

// ============================================================================
// Relay Chain Block Handlers
// ============================================================================

async fn handle_events_use_rc_block(
    state: AppState,
    pallet_id: String,
    params: PalletQueryParams,
) -> Result<Response, PalletError> {
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(PalletError::UseRcBlockNotSupported);
    }

    if state.get_relay_chain_client().is_none() {
        return Err(PalletError::RelayChainNotConfigured);
    }

    let rc_block_id = params
        .at
        .as_ref()
        .ok_or(PalletError::AtParameterRequired)?
        .parse::<utils::BlockId>()?;

    let rc_resolved_block = utils::resolve_block_with_rpc(
        state.get_relay_chain_rpc_client().expect("checked above"),
        state.get_relay_chain_rpc().expect("checked above"),
        Some(rc_block_id),
    )
    .await?;

    let ah_blocks = find_ah_blocks_in_rc_block(&state, &rc_resolved_block).await?;

    if ah_blocks.is_empty() {
        return Ok((StatusCode::OK, Json(Vec::<PalletEventsResponse>::new())).into_response());
    }

    let mut results = Vec::new();
    let rc_block_hash = rc_resolved_block.hash.clone();
    let rc_block_number = rc_resolved_block.number.to_string();

    for ah_block in &ah_blocks {
        let client_at_block = state.client.at_block(ah_block.number).await?;

        let at = AtResponse {
            hash: ah_block.hash.clone(),
            height: ah_block.number.to_string(),
        };

        let ah_timestamp = utils::fetch_block_timestamp(&client_at_block).await;

        let rc_fields = RcBlockFields {
            rc_block_hash: Some(rc_block_hash.clone()),
            rc_block_number: Some(rc_block_number.clone()),
            ah_timestamp,
        };

        let metadata = client_at_block.metadata();

        let response =
            extract_events_from_metadata(&metadata, &pallet_id, at, params.only_ids, rc_fields)?;

        results.push(response);
    }

    Ok((StatusCode::OK, Json(results)).into_response())
}

async fn handle_event_item_use_rc_block(
    state: AppState,
    pallet_id: String,
    event_item_id: String,
    params: PalletItemQueryParams,
) -> Result<Response, PalletError> {
    if state.chain_info.chain_type != ChainType::AssetHub {
        return Err(PalletError::UseRcBlockNotSupported);
    }

    if state.get_relay_chain_client().is_none() {
        return Err(PalletError::RelayChainNotConfigured);
    }

    let rc_block_id = params
        .at
        .as_ref()
        .ok_or(PalletError::AtParameterRequired)?
        .parse::<utils::BlockId>()?;

    let rc_resolved_block = utils::resolve_block_with_rpc(
        state.get_relay_chain_rpc_client().expect("checked above"),
        state.get_relay_chain_rpc().expect("checked above"),
        Some(rc_block_id),
    )
    .await?;

    let ah_blocks = find_ah_blocks_in_rc_block(&state, &rc_resolved_block).await?;

    if ah_blocks.is_empty() {
        return Ok((StatusCode::OK, Json(Vec::<PalletEventItemResponse>::new())).into_response());
    }

    let mut results = Vec::new();
    let rc_block_hash = rc_resolved_block.hash.clone();
    let rc_block_number = rc_resolved_block.number.to_string();

    for ah_block in &ah_blocks {
        let client_at_block = state.client.at_block(ah_block.number).await?;

        let at = AtResponse {
            hash: ah_block.hash.clone(),
            height: ah_block.number.to_string(),
        };

        let ah_timestamp = utils::fetch_block_timestamp(&client_at_block).await;

        let rc_fields = RcBlockFields {
            rc_block_hash: Some(rc_block_hash.clone()),
            rc_block_number: Some(rc_block_number.clone()),
            ah_timestamp,
        };

        let metadata = client_at_block.metadata();

        let response = extract_event_item_from_metadata(
            &metadata,
            &pallet_id,
            &event_item_id,
            at,
            params.metadata,
            rc_fields,
        )?;

        results.push(response);
    }

    Ok((StatusCode::OK, Json(results)).into_response())
}

// ============================================================================
// Metadata Extraction - Using Subxt's normalized metadata API
// ============================================================================

fn find_pallet<'a>(
    metadata: &'a Metadata,
    pallet_id: &str,
) -> Option<subxt_metadata::PalletMetadata<'a>> {
    if let Ok(index) = pallet_id.parse::<u8>() {
        return metadata
            .pallets()
            .find(|pallet| pallet.call_index() == index);
    }

    let pallet_id_lower = pallet_id.to_lowercase();
    metadata
        .pallets()
        .find(|pallet| pallet.name().to_lowercase() == pallet_id_lower)
}

/// Extract events from subxt's unified Metadata.
fn extract_events_from_metadata(
    metadata: &Metadata,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletEventsResponse, PalletError> {
    let pallet = find_pallet(metadata, pallet_id)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet_name = pallet.name().to_string();
    let pallet_index = pallet.call_index();

    let types = metadata.types();

    let items = match pallet.event_variants() {
        Some(variants) => {
            if only_ids {
                EventsItems::OnlyIds(variants.iter().map(|v| v.name.clone()).collect())
            } else {
                EventsItems::Full(
                    variants
                        .iter()
                        .map(|variant| {
                            let fields: Vec<EventField> = variant
                                .fields
                                .iter()
                                .map(|f| EventField {
                                    name: f.name.clone(),
                                    ty: f.ty.id.to_string(),
                                    type_name: f.type_name.clone(),
                                    docs: f.docs.clone(),
                                })
                                .collect();

                            let args: Vec<String> = variant
                                .fields
                                .iter()
                                .map(|f| resolve_type_name(types, f.ty.id))
                                .collect();

                            EventItemMetadata {
                                name: variant.name.clone(),
                                fields,
                                index: variant.index.to_string(),
                                docs: variant.docs.clone(),
                                args,
                            }
                        })
                        .collect(),
                )
            }
        }
        None => {
            if only_ids {
                EventsItems::OnlyIds(vec![])
            } else {
                EventsItems::Full(vec![])
            }
        }
    };

    Ok(PalletEventsResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: pallet_index.to_string(),
        items,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

/// Extract a single event item from subxt's unified Metadata.
fn extract_event_item_from_metadata(
    metadata: &Metadata,
    pallet_id: &str,
    event_item_id: &str,
    at: AtResponse,
    include_metadata: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletEventItemResponse, PalletError> {
    let pallet = find_pallet(metadata, pallet_id)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet_name = pallet.name().to_string();
    let pallet_index = pallet.call_index();

    let event_variants = pallet
        .event_variants()
        .ok_or_else(|| PalletError::EventNotFound(event_item_id.to_string()))?;

    let event_item_id_lower = event_item_id.to_lowercase();
    let event_variant = event_variants
        .iter()
        .find(|v| v.name.to_lowercase() == event_item_id_lower)
        .ok_or_else(|| PalletError::EventNotFound(event_item_id.to_string()))?;

    let event_name = event_variant.name.clone();

    let types = metadata.types();

    let event_metadata = if include_metadata {
        let fields: Vec<EventField> = event_variant
            .fields
            .iter()
            .map(|f| EventField {
                name: f.name.clone(),
                ty: f.ty.id.to_string(),
                type_name: f.type_name.clone(),
                docs: f.docs.clone(),
            })
            .collect();

        let args: Vec<String> = event_variant
            .fields
            .iter()
            .map(|f| resolve_type_name(types, f.ty.id))
            .collect();

        Some(EventItemMetadata {
            name: event_variant.name.clone(),
            fields,
            index: event_variant.index.to_string(),
            docs: event_variant.docs.clone(),
            args,
        })
    } else {
        None
    };

    Ok(PalletEventItemResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: pallet_index.to_string(),
        event_item: event_name.to_lower_camel_case(),
        metadata: event_metadata,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

// ============================================================================
// RC (Relay Chain) Handlers
// ============================================================================

/// Handler for GET `/rc/pallets/{palletId}/events`
///
/// Returns events from the relay chain's pallet metadata.
#[utoipa::path(
    get,
    path = "/v1/rc/pallets/{palletId}/events",
    tag = "rc",
    summary = "RC pallet events",
    description = "Returns all events defined in a relay chain pallet.",
    params(
        ("palletId" = String, Path, description = "Name or index of the pallet"),
        ("at" = Option<String>, Query, description = "Block hash or number to query at"),
        ("onlyIds" = Option<bool>, Query, description = "Only return event names")
    ),
    responses(
        (status = 200, description = "Relay chain pallet events", body = Object),
        (status = 400, description = "Invalid pallet"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn rc_pallet_events(
    State(state): State<AppState>,
    Path(pallet_id): Path<String>,
    Query(params): Query<RcPalletQueryParams>,
) -> Result<Response, PalletError> {
    let relay_client = state
        .get_relay_chain_client()
        .ok_or(PalletError::RelayChainNotConfigured)?;
    let relay_rpc_client = state
        .get_relay_chain_rpc_client()
        .ok_or(PalletError::RelayChainNotConfigured)?;
    let relay_rpc = state
        .get_relay_chain_rpc()
        .ok_or(PalletError::RelayChainNotConfigured)?;

    let block_id = params
        .at
        .as_ref()
        .map(|s: &String| s.parse::<utils::BlockId>())
        .transpose()?;
    let resolved = utils::resolve_block_with_rpc(relay_rpc_client, relay_rpc, block_id).await?;

    let client_at_block = relay_client.at_block(resolved.number).await?;
    let metadata = client_at_block.metadata();

    let at = AtResponse {
        hash: resolved.hash.clone(),
        height: resolved.number.to_string(),
    };

    let response = extract_events_from_metadata(
        &metadata,
        &pallet_id,
        at,
        params.only_ids,
        RcBlockFields::default(),
    )?;

    Ok((StatusCode::OK, Json(response)).into_response())
}

/// Handler for GET `/rc/pallets/{palletId}/events/{eventItemId}`
///
/// Returns a specific event from the relay chain's pallet metadata.
#[utoipa::path(
    get,
    path = "/v1/rc/pallets/{palletId}/events/{eventItemId}",
    tag = "rc",
    summary = "RC pallet event details",
    description = "Returns metadata for a specific event in a relay chain pallet.",
    params(
        ("palletId" = String, Path, description = "Name or index of the pallet"),
        ("eventItemId" = String, Path, description = "Event name"),
        ("at" = Option<String>, Query, description = "Block hash or number to query at"),
        ("metadata" = Option<bool>, Query, description = "Include full event metadata")
    ),
    responses(
        (status = 200, description = "Relay chain event details", body = Object),
        (status = 404, description = "Pallet or event not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn rc_pallet_event_item(
    State(state): State<AppState>,
    Path((pallet_id, event_item_id)): Path<(String, String)>,
    Query(params): Query<RcPalletItemQueryParams>,
) -> Result<Response, PalletError> {
    let relay_client = state
        .get_relay_chain_client()
        .ok_or(PalletError::RelayChainNotConfigured)?;
    let relay_rpc_client = state
        .get_relay_chain_rpc_client()
        .ok_or(PalletError::RelayChainNotConfigured)?;
    let relay_rpc = state
        .get_relay_chain_rpc()
        .ok_or(PalletError::RelayChainNotConfigured)?;

    let block_id = params
        .at
        .as_ref()
        .map(|s: &String| s.parse::<utils::BlockId>())
        .transpose()?;
    let resolved = utils::resolve_block_with_rpc(relay_rpc_client, relay_rpc, block_id).await?;

    let client_at_block = relay_client.at_block(resolved.number).await?;
    let metadata = client_at_block.metadata();

    let at = AtResponse {
        hash: resolved.hash.clone(),
        height: resolved.number.to_string(),
    };

    let response = extract_event_item_from_metadata(
        &metadata,
        &pallet_id,
        &event_item_id,
        at,
        params.metadata,
        RcBlockFields::default(),
    )?;

    Ok((StatusCode::OK, Json(response)).into_response())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_events_query_params_defaults() {
        let json = r#"{"at": "123"}"#;
        let params: PalletQueryParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.at, Some("123".to_string()));
        assert!(!params.only_ids);
        assert!(!params.use_rc_block);
    }

    #[test]
    fn test_event_item_query_params_defaults() {
        let json = r#"{"at": "456"}"#;
        let params: PalletItemQueryParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.at, Some("456".to_string()));
        assert!(!params.metadata);
        assert!(!params.use_rc_block);
    }

    #[test]
    fn test_event_item_metadata_serialization() {
        let metadata = EventItemMetadata {
            name: "Transfer".to_string(),
            fields: vec![EventField {
                name: Some("from".to_string()),
                ty: "0".to_string(),
                type_name: Some("AccountId".to_string()),
                docs: vec![],
            }],
            index: "0".to_string(),
            docs: vec!["A transfer event.".to_string()],
            args: vec!["AccountId".to_string()],
        };

        let json = serde_json::to_string(&metadata).unwrap();
        assert!(json.contains("\"name\":\"Transfer\""));
        assert!(json.contains("\"index\":\"0\""));
    }

    #[test]
    fn test_pallet_events_response_serialization() {
        let response = PalletEventsResponse {
            at: AtResponse {
                hash: "0xabc".to_string(),
                height: "100".to_string(),
            },
            pallet: "balances".to_string(),
            pallet_index: "5".to_string(),
            items: EventsItems::OnlyIds(vec!["Transfer".to_string(), "Deposit".to_string()]),
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"pallet\":\"balances\""));
        assert!(json.contains("\"palletIndex\":\"5\""));
        // RC block fields should not be present when None
        assert!(!json.contains("rcBlockHash"));
        assert!(!json.contains("rcBlockNumber"));
        assert!(!json.contains("ahTimestamp"));
    }

    #[test]
    fn test_pallet_events_response_with_rc_block_serialization() {
        let response = PalletEventsResponse {
            at: AtResponse {
                hash: "0xabc".to_string(),
                height: "100".to_string(),
            },
            pallet: "balances".to_string(),
            pallet_index: "5".to_string(),
            items: EventsItems::OnlyIds(vec!["Transfer".to_string()]),
            rc_block_hash: Some("0xrc123".to_string()),
            rc_block_number: Some("5000".to_string()),
            ah_timestamp: Some("1642694400".to_string()),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"rcBlockHash\":\"0xrc123\""));
        assert!(json.contains("\"rcBlockNumber\":\"5000\""));
        assert!(json.contains("\"ahTimestamp\":\"1642694400\""));
    }

    #[test]
    fn test_pallet_event_item_response_serialization() {
        let response = PalletEventItemResponse {
            at: AtResponse {
                hash: "0xdef".to_string(),
                height: "200".to_string(),
            },
            pallet: "balances".to_string(),
            pallet_index: "5".to_string(),
            event_item: "transfer".to_string(),
            metadata: None,
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"eventItem\":\"transfer\""));
        assert!(!json.contains("\"metadata\""));
        assert!(!json.contains("rcBlockHash"));
    }

    #[test]
    fn test_pallet_event_item_response_with_rc_block_serialization() {
        let response = PalletEventItemResponse {
            at: AtResponse {
                hash: "0xdef".to_string(),
                height: "200".to_string(),
            },
            pallet: "balances".to_string(),
            pallet_index: "5".to_string(),
            event_item: "transfer".to_string(),
            metadata: None,
            rc_block_hash: Some("0xrc456".to_string()),
            rc_block_number: Some("6000".to_string()),
            ah_timestamp: Some("1642694500".to_string()),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"rcBlockHash\":\"0xrc456\""));
        assert!(json.contains("\"rcBlockNumber\":\"6000\""));
        assert!(json.contains("\"ahTimestamp\":\"1642694500\""));
    }

    #[test]
    fn test_events_items_full_serialization() {
        let items = EventsItems::Full(vec![EventItemMetadata {
            name: "Test".to_string(),
            fields: vec![],
            index: "0".to_string(),
            docs: vec![],
            args: vec![],
        }]);

        let json = serde_json::to_string(&items).unwrap();
        assert!(json.contains("\"name\":\"Test\""));
    }

    #[test]
    fn test_events_items_only_ids_serialization() {
        let items = EventsItems::OnlyIds(vec!["Event1".to_string(), "Event2".to_string()]);

        let json = serde_json::to_string(&items).unwrap();
        assert_eq!(json, r#"["Event1","Event2"]"#);
    }
}
