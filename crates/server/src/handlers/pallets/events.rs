//! Handler for the `/pallets/{palletId}/events` endpoints.
//!
//! This module provides endpoints for querying event metadata from pallets.
//! Uses Subxt's metadata API which normalizes all metadata versions internally.

#![allow(clippy::result_large_err)]

use crate::handlers::pallets::common::{AtResponse, PalletError};
use crate::state::AppState;
use crate::utils;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use heck::ToLowerCamelCase;
use serde::{Deserialize, Serialize};
use subxt::Metadata;

// ============================================================================
// Request/Response Types
// ============================================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PalletEventsQueryParams {
    pub at: Option<String>,
    #[serde(default)]
    pub only_ids: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PalletEventItemQueryParams {
    pub at: Option<String>,
    #[serde(default)]
    pub metadata: bool,
}

/// Response for `/pallets/{palletId}/events`
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PalletEventsResponse {
    pub at: AtResponse,
    pub pallet: String,
    pub pallet_index: String,
    pub items: EventsItems,
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
// Internal Types
// ============================================================================

struct PalletEventsInfo {
    index: u8,
    events: Vec<EventItemMetadata>,
}

// ============================================================================
// Main Handlers
// ============================================================================

/// Handler for GET `/pallets/{palletId}/events`
///
/// Returns all events defined in a pallet.
pub async fn get_pallet_events(
    State(state): State<AppState>,
    Path(pallet_id): Path<String>,
    Query(params): Query<PalletEventsQueryParams>,
) -> Result<Response, PalletError> {
    let block_id = params
        .at
        .as_ref()
        .map(|s| s.parse::<utils::BlockId>())
        .transpose()?;
    let resolved = utils::resolve_block(&state, block_id).await?;

    // Get client at block - Subxt normalizes all metadata versions
    let client_at_block = state.client.at_block(resolved.number).await?;
    let metadata = client_at_block.metadata();

    let pallet_info = extract_pallet_events(&metadata, &pallet_id)?;

    let at = AtResponse {
        hash: resolved.hash.clone(),
        height: resolved.number.to_string(),
    };

    let items = if params.only_ids {
        EventsItems::OnlyIds(pallet_info.events.iter().map(|e| e.name.clone()).collect())
    } else {
        EventsItems::Full(pallet_info.events)
    };

    Ok((
        StatusCode::OK,
        Json(PalletEventsResponse {
            at,
            pallet: pallet_id.to_lowercase(),
            pallet_index: pallet_info.index.to_string(),
            items,
        }),
    )
        .into_response())
}

/// Handler for GET `/pallets/{palletId}/events/{eventItemId}`
///
/// Returns metadata for a specific event in a pallet.
pub async fn get_pallet_event_item(
    State(state): State<AppState>,
    Path((pallet_id, event_item_id)): Path<(String, String)>,
    Query(params): Query<PalletEventItemQueryParams>,
) -> Result<Response, PalletError> {
    let block_id = params
        .at
        .as_ref()
        .map(|s| s.parse::<utils::BlockId>())
        .transpose()?;
    let resolved = utils::resolve_block(&state, block_id).await?;

    // Get client at block - Subxt normalizes all metadata versions
    let client_at_block = state.client.at_block(resolved.number).await?;
    let metadata = client_at_block.metadata();

    let pallet_info = extract_pallet_events(&metadata, &pallet_id)?;

    let event = pallet_info
        .events
        .iter()
        .find(|e| e.name.to_lowercase() == event_item_id.to_lowercase())
        .ok_or_else(|| PalletError::EventNotFound(event_item_id.clone()))?;

    let at = AtResponse {
        hash: resolved.hash.clone(),
        height: resolved.number.to_string(),
    };

    let metadata_field = if params.metadata {
        Some(event.clone())
    } else {
        None
    };

    Ok((
        StatusCode::OK,
        Json(PalletEventItemResponse {
            at,
            pallet: pallet_id.to_lowercase(),
            pallet_index: pallet_info.index.to_string(),
            event_item: event.name.to_lower_camel_case(),
            metadata: metadata_field,
        }),
    )
        .into_response())
}

// ============================================================================
// Metadata Extraction - Using Subxt's normalized metadata API
// ============================================================================

/// Extract pallet events using Subxt's metadata API.
/// Subxt normalizes all metadata versions (V9-V15+) into a unified format.
fn extract_pallet_events(
    metadata: &Metadata,
    pallet_id: &str,
) -> Result<PalletEventsInfo, PalletError> {
    // Try to find pallet by index first, then by name
    let pallet = if let Ok(index) = pallet_id.parse::<u8>() {
        metadata.pallets().find(|p| p.call_index() == index)
    } else {
        metadata.pallet_by_name(pallet_id)
    };

    let pallet = pallet.ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    // Get event variants - Subxt provides this across all metadata versions
    let events: Vec<EventItemMetadata> = match pallet.event_variants() {
        Some(variants) => variants
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

                // Build args list from field type names (for backwards compatibility)
                let args: Vec<String> = variant
                    .fields
                    .iter()
                    .filter_map(|f| f.type_name.clone())
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
        None => vec![], // Pallet has no events
    };

    Ok(PalletEventsInfo {
        index: pallet.call_index(),
        events,
    })
}

// ============================================================================
// RC (Relay Chain) Handlers
// ============================================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RcEventsQueryParams {
    pub at: Option<String>,
    #[serde(default)]
    pub only_ids: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RcEventItemQueryParams {
    pub at: Option<String>,
    #[serde(default)]
    pub metadata: bool,
}

/// Handler for GET `/rc/pallets/{palletId}/events`
///
/// Returns events from the relay chain's pallet metadata.
pub async fn rc_pallet_events(
    State(state): State<AppState>,
    Path(pallet_id): Path<String>,
    Query(params): Query<RcEventsQueryParams>,
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
        .map(|s| s.parse::<utils::BlockId>())
        .transpose()?;
    let resolved = utils::resolve_block_with_rpc(relay_rpc_client, relay_rpc, block_id).await?;

    let client_at_block = relay_client.at_block(resolved.number).await?;
    let metadata = client_at_block.metadata();

    let pallet_info = extract_pallet_events(&metadata, &pallet_id)?;

    let at = AtResponse {
        hash: resolved.hash.clone(),
        height: resolved.number.to_string(),
    };

    let items = if params.only_ids {
        EventsItems::OnlyIds(pallet_info.events.iter().map(|e| e.name.clone()).collect())
    } else {
        EventsItems::Full(pallet_info.events)
    };

    Ok((
        StatusCode::OK,
        Json(PalletEventsResponse {
            at,
            pallet: pallet_id.to_lowercase(),
            pallet_index: pallet_info.index.to_string(),
            items,
        }),
    )
        .into_response())
}

/// Handler for GET `/rc/pallets/{palletId}/events/{eventItemId}`
///
/// Returns a specific event from the relay chain's pallet metadata.
pub async fn rc_pallet_event_item(
    State(state): State<AppState>,
    Path((pallet_id, event_item_id)): Path<(String, String)>,
    Query(params): Query<RcEventItemQueryParams>,
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
        .map(|s| s.parse::<utils::BlockId>())
        .transpose()?;
    let resolved = utils::resolve_block_with_rpc(relay_rpc_client, relay_rpc, block_id).await?;

    let client_at_block = relay_client.at_block(resolved.number).await?;
    let metadata = client_at_block.metadata();

    let pallet_info = extract_pallet_events(&metadata, &pallet_id)?;

    let event = pallet_info
        .events
        .iter()
        .find(|e| e.name.to_lowercase() == event_item_id.to_lowercase())
        .ok_or_else(|| PalletError::EventNotFound(event_item_id.clone()))?;

    let at = AtResponse {
        hash: resolved.hash.clone(),
        height: resolved.number.to_string(),
    };

    let metadata_field = if params.metadata {
        Some(event.clone())
    } else {
        None
    };

    Ok((
        StatusCode::OK,
        Json(PalletEventItemResponse {
            at,
            pallet: pallet_id.to_lowercase(),
            pallet_index: pallet_info.index.to_string(),
            event_item: event.name.to_lower_camel_case(),
            metadata: metadata_field,
        }),
    )
        .into_response())
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
        let params: PalletEventsQueryParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.at, Some("123".to_string()));
        assert!(!params.only_ids);
    }

    #[test]
    fn test_event_item_query_params_defaults() {
        let json = r#"{"at": "456"}"#;
        let params: PalletEventItemQueryParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.at, Some("456".to_string()));
        assert!(!params.metadata);
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
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"pallet\":\"balances\""));
        assert!(json.contains("\"palletIndex\":\"5\""));
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
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"eventItem\":\"transfer\""));
        assert!(!json.contains("\"metadata\""));
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
