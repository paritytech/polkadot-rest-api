//! Handler for the `/pallets/{palletId}/events` endpoints.
//!
//! This module provides endpoints for querying event metadata from pallets.

use crate::handlers::pallets::common::{AtResponse, PalletError};
use crate::state::AppState;
use crate::utils;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use frame_metadata::{RuntimeMetadata, RuntimeMetadataPrefixed, decode_different::DecodeDifferent};
use heck::ToLowerCamelCase;
use serde::{Deserialize, Serialize};

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
    // Create client at the specified block
    let client_at_block = match params.at {
        None => state.client.at_current_block().await?,
        Some(ref at_str) => {
            let block_id = at_str.parse::<utils::BlockId>()?;
            match block_id {
                utils::BlockId::Hash(hash) => state.client.at_block(hash).await?,
                utils::BlockId::Number(number) => state.client.at_block(number).await?,
            }
        }
    };

    let at = AtResponse {
        hash: format!("{:#x}", client_at_block.block_hash()),
        height: client_at_block.block_number().to_string(),
    };

    // Fetch metadata via RPC
    let metadata_hex: String = state
        .rpc_client
        .request(
            "state_getMetadata",
            subxt_rpcs::client::rpc_params![&at.hash],
        )
        .await
        .map_err(|_| PalletError::MetadataFetchFailed)?;

    let hex_str = metadata_hex.strip_prefix("0x").unwrap_or(&metadata_hex);
    let metadata_bytes = hex::decode(hex_str)
        .map_err(|e| PalletError::MetadataDecodeFailed(format!("hex decode: {}", e)))?;

    let metadata: RuntimeMetadataPrefixed = parity_scale_codec::Decode::decode(&mut &metadata_bytes[..])
        .map_err(|e| PalletError::MetadataDecodeFailed(format!("SCALE decode: {}", e)))?;

    // Extract events based on metadata version
    extract_pallet_events(&metadata, &pallet_id, at, params.only_ids)
}

/// Handler for GET `/pallets/{palletId}/events/{eventItemId}`
///
/// Returns metadata for a specific event in a pallet.
pub async fn get_pallet_event_item(
    State(state): State<AppState>,
    Path((pallet_id, event_item_id)): Path<(String, String)>,
    Query(params): Query<PalletEventItemQueryParams>,
) -> Result<Response, PalletError> {
    // Create client at the specified block
    let client_at_block = match params.at {
        None => state.client.at_current_block().await?,
        Some(ref at_str) => {
            let block_id = at_str.parse::<utils::BlockId>()?;
            match block_id {
                utils::BlockId::Hash(hash) => state.client.at_block(hash).await?,
                utils::BlockId::Number(number) => state.client.at_block(number).await?,
            }
        }
    };

    let at = AtResponse {
        hash: format!("{:#x}", client_at_block.block_hash()),
        height: client_at_block.block_number().to_string(),
    };

    // Fetch metadata via RPC
    let metadata_hex: String = state
        .rpc_client
        .request(
            "state_getMetadata",
            subxt_rpcs::client::rpc_params![&at.hash],
        )
        .await
        .map_err(|_| PalletError::MetadataFetchFailed)?;

    let hex_str = metadata_hex.strip_prefix("0x").unwrap_or(&metadata_hex);
    let metadata_bytes = hex::decode(hex_str)
        .map_err(|e| PalletError::MetadataDecodeFailed(format!("hex decode: {}", e)))?;

    let metadata: RuntimeMetadataPrefixed = parity_scale_codec::Decode::decode(&mut &metadata_bytes[..])
        .map_err(|e| PalletError::MetadataDecodeFailed(format!("SCALE decode: {}", e)))?;

    // Extract specific event based on metadata version
    extract_pallet_event_item(&metadata, &pallet_id, &event_item_id, at, params.metadata)
}

// ============================================================================
// Metadata Extraction - Main Dispatcher
// ============================================================================

fn extract_pallet_events(
    metadata: &RuntimeMetadataPrefixed,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
) -> Result<Response, PalletError> {
    match &metadata.1 {
        RuntimeMetadata::V9(m) => extract_events_v9_v13(&m.modules, pallet_id, at, only_ids),
        RuntimeMetadata::V10(m) => extract_events_v9_v13(&m.modules, pallet_id, at, only_ids),
        RuntimeMetadata::V11(m) => extract_events_v9_v13(&m.modules, pallet_id, at, only_ids),
        RuntimeMetadata::V12(m) => extract_events_v9_v13(&m.modules, pallet_id, at, only_ids),
        RuntimeMetadata::V13(m) => extract_events_v9_v13(&m.modules, pallet_id, at, only_ids),
        RuntimeMetadata::V14(m) => extract_events_v14(m, pallet_id, at, only_ids),
        RuntimeMetadata::V15(m) => extract_events_v15(m, pallet_id, at, only_ids),
        _ => Err(PalletError::PalletNotFound(pallet_id.to_string())),
    }
}

fn extract_pallet_event_item(
    metadata: &RuntimeMetadataPrefixed,
    pallet_id: &str,
    event_item_id: &str,
    at: AtResponse,
    include_metadata: bool,
) -> Result<Response, PalletError> {
    match &metadata.1 {
        RuntimeMetadata::V9(m) => extract_event_item_v9_v13(&m.modules, pallet_id, event_item_id, at, include_metadata),
        RuntimeMetadata::V10(m) => extract_event_item_v9_v13(&m.modules, pallet_id, event_item_id, at, include_metadata),
        RuntimeMetadata::V11(m) => extract_event_item_v9_v13(&m.modules, pallet_id, event_item_id, at, include_metadata),
        RuntimeMetadata::V12(m) => extract_event_item_v9_v13(&m.modules, pallet_id, event_item_id, at, include_metadata),
        RuntimeMetadata::V13(m) => extract_event_item_v9_v13(&m.modules, pallet_id, event_item_id, at, include_metadata),
        RuntimeMetadata::V14(m) => extract_event_item_v14(m, pallet_id, event_item_id, at, include_metadata),
        RuntimeMetadata::V15(m) => extract_event_item_v15(m, pallet_id, event_item_id, at, include_metadata),
        _ => Err(PalletError::PalletNotFound(pallet_id.to_string())),
    }
}

// ============================================================================
// V9-V13 Extraction (Legacy Metadata)
// ============================================================================

fn extract_str<'a>(s: &'a DecodeDifferent<&'static str, String>) -> &'a str {
    match s {
        DecodeDifferent::Decoded(v) => v.as_str(),
        DecodeDifferent::Encode(s) => s,
    }
}

fn extract_docs(docs: &DecodeDifferent<&'static [&'static str], Vec<String>>) -> Vec<String> {
    match docs {
        DecodeDifferent::Decoded(v) => v.clone(),
        DecodeDifferent::Encode(s) => s.iter().map(|s| s.to_string()).collect(),
    }
}

/// Find a pallet by name (case-insensitive) or index in V9-V13 metadata.
fn find_pallet_v9_v13<M>(
    modules: &DecodeDifferent<&'static [M], Vec<M>>,
    pallet_id: &str,
) -> Option<(String, u8, usize)>
where
    M: ModuleMetadataTrait,
{
    let DecodeDifferent::Decoded(modules) = modules else {
        return None;
    };

    let pallet_id_lower = pallet_id.to_lowercase();

    // Try to parse as index first
    if let Ok(index) = pallet_id.parse::<u8>() {
        for (i, module) in modules.iter().enumerate() {
            if module.get_index() == index {
                return Some((module.get_name().to_string(), index, i));
            }
        }
    }

    // Otherwise search by name (case-insensitive)
    for (i, module) in modules.iter().enumerate() {
        if module.get_name().to_lowercase() == pallet_id_lower {
            return Some((module.get_name().to_string(), module.get_index(), i));
        }
    }

    None
}

/// Trait to abstract over different module metadata versions.
trait ModuleMetadataTrait {
    fn get_name(&self) -> &str;
    fn get_index(&self) -> u8;
    fn get_events(&self) -> Option<Vec<LegacyEventMetadata>>;
}

/// Simplified event metadata for V9-V13.
struct LegacyEventMetadata {
    name: String,
    arguments: Vec<String>,
    docs: Vec<String>,
}

impl ModuleMetadataTrait for frame_metadata::v12::ModuleMetadata {
    fn get_name(&self) -> &str {
        extract_str(&self.name)
    }

    fn get_index(&self) -> u8 {
        self.index
    }

    fn get_events(&self) -> Option<Vec<LegacyEventMetadata>> {
        let events = self.event.as_ref()?;
        let DecodeDifferent::Decoded(events) = events else {
            return None;
        };
        Some(
            events
                .iter()
                .map(|e| {
                    let args = match &e.arguments {
                        DecodeDifferent::Decoded(v) => v.clone(),
                        DecodeDifferent::Encode(s) => s.iter().map(|s| s.to_string()).collect(),
                    };
                    LegacyEventMetadata {
                        name: extract_str(&e.name).to_string(),
                        arguments: args,
                        docs: extract_docs(&e.documentation),
                    }
                })
                .collect(),
        )
    }
}

impl ModuleMetadataTrait for frame_metadata::v13::ModuleMetadata {
    fn get_name(&self) -> &str {
        extract_str(&self.name)
    }

    fn get_index(&self) -> u8 {
        self.index
    }

    fn get_events(&self) -> Option<Vec<LegacyEventMetadata>> {
        let events = self.event.as_ref()?;
        let DecodeDifferent::Decoded(events) = events else {
            return None;
        };
        Some(
            events
                .iter()
                .map(|e| {
                    let args = match &e.arguments {
                        DecodeDifferent::Decoded(v) => v.clone(),
                        DecodeDifferent::Encode(s) => s.iter().map(|s| s.to_string()).collect(),
                    };
                    LegacyEventMetadata {
                        name: extract_str(&e.name).to_string(),
                        arguments: args,
                        docs: extract_docs(&e.documentation),
                    }
                })
                .collect(),
        )
    }
}

// V9, V10, V11 share similar structure with V12
impl ModuleMetadataTrait for frame_metadata::v9::ModuleMetadata {
    fn get_name(&self) -> &str {
        extract_str(&self.name)
    }

    fn get_index(&self) -> u8 {
        // V9 doesn't have index field, use position
        0
    }

    fn get_events(&self) -> Option<Vec<LegacyEventMetadata>> {
        let events = self.event.as_ref()?;
        let DecodeDifferent::Decoded(events) = events else {
            return None;
        };
        Some(
            events
                .iter()
                .map(|e| {
                    let args = match &e.arguments {
                        DecodeDifferent::Decoded(v) => v.clone(),
                        DecodeDifferent::Encode(s) => s.iter().map(|s| s.to_string()).collect(),
                    };
                    LegacyEventMetadata {
                        name: extract_str(&e.name).to_string(),
                        arguments: args,
                        docs: extract_docs(&e.documentation),
                    }
                })
                .collect(),
        )
    }
}

impl ModuleMetadataTrait for frame_metadata::v10::ModuleMetadata {
    fn get_name(&self) -> &str {
        extract_str(&self.name)
    }

    fn get_index(&self) -> u8 {
        // V10 doesn't have index field, use position
        0
    }

    fn get_events(&self) -> Option<Vec<LegacyEventMetadata>> {
        let events = self.event.as_ref()?;
        let DecodeDifferent::Decoded(events) = events else {
            return None;
        };
        Some(
            events
                .iter()
                .map(|e| {
                    let args = match &e.arguments {
                        DecodeDifferent::Decoded(v) => v.clone(),
                        DecodeDifferent::Encode(s) => s.iter().map(|s| s.to_string()).collect(),
                    };
                    LegacyEventMetadata {
                        name: extract_str(&e.name).to_string(),
                        arguments: args,
                        docs: extract_docs(&e.documentation),
                    }
                })
                .collect(),
        )
    }
}

impl ModuleMetadataTrait for frame_metadata::v11::ModuleMetadata {
    fn get_name(&self) -> &str {
        extract_str(&self.name)
    }

    fn get_index(&self) -> u8 {
        // V11 doesn't have index field, use position
        0
    }

    fn get_events(&self) -> Option<Vec<LegacyEventMetadata>> {
        let events = self.event.as_ref()?;
        let DecodeDifferent::Decoded(events) = events else {
            return None;
        };
        Some(
            events
                .iter()
                .map(|e| {
                    let args = match &e.arguments {
                        DecodeDifferent::Decoded(v) => v.clone(),
                        DecodeDifferent::Encode(s) => s.iter().map(|s| s.to_string()).collect(),
                    };
                    LegacyEventMetadata {
                        name: extract_str(&e.name).to_string(),
                        arguments: args,
                        docs: extract_docs(&e.documentation),
                    }
                })
                .collect(),
        )
    }
}

fn extract_events_v9_v13<M>(
    modules: &DecodeDifferent<&'static [M], Vec<M>>,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
) -> Result<Response, PalletError>
where
    M: ModuleMetadataTrait,
{
    let (pallet_name, pallet_index, module_idx) =
        find_pallet_v9_v13(modules, pallet_id).ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let DecodeDifferent::Decoded(modules_vec) = modules else {
        return Err(PalletError::PalletNotFound(pallet_id.to_string()));
    };

    let module = &modules_vec[module_idx];
    let events = module.get_events().unwrap_or_default();

    // If the pallet has no events, return an error (matching Sidecar behavior)
    if events.is_empty() {
        return Err(PalletError::NoEventsInPallet(pallet_id.to_string()));
    }

    let items = if only_ids {
        EventsItems::OnlyIds(events.iter().map(|e| e.name.clone()).collect())
    } else {
        EventsItems::Full(
            events
                .iter()
                .enumerate()
                .map(|(idx, e)| EventItemMetadata {
                    name: e.name.clone(),
                    fields: e
                        .arguments
                        .iter()
                        .enumerate()
                        .map(|(i, arg_ty)| EventField {
                            name: Some(format!("arg{}", i)),
                            ty: arg_ty.clone(),
                            type_name: Some(arg_ty.clone()),
                            docs: vec![],
                        })
                        .collect(),
                    index: idx.to_string(),
                    docs: e.docs.clone(),
                    args: e.arguments.clone(),
                })
                .collect(),
        )
    };

    Ok((
        StatusCode::OK,
        Json(PalletEventsResponse {
            at,
            pallet: pallet_name.to_lowercase(),
            pallet_index: pallet_index.to_string(),
            items,
        }),
    )
        .into_response())
}

fn extract_event_item_v9_v13<M>(
    modules: &DecodeDifferent<&'static [M], Vec<M>>,
    pallet_id: &str,
    event_item_id: &str,
    at: AtResponse,
    include_metadata: bool,
) -> Result<Response, PalletError>
where
    M: ModuleMetadataTrait,
{
    let (pallet_name, pallet_index, module_idx) =
        find_pallet_v9_v13(modules, pallet_id).ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let DecodeDifferent::Decoded(modules_vec) = modules else {
        return Err(PalletError::PalletNotFound(pallet_id.to_string()));
    };

    let module = &modules_vec[module_idx];
    let events = module.get_events().unwrap_or_default();

    let event_id_lower = event_item_id.to_lowercase();
    let (idx, event) = events
        .iter()
        .enumerate()
        .find(|(_, e)| e.name.to_lowercase() == event_id_lower)
        .ok_or_else(|| PalletError::EventNotFound(event_item_id.to_string()))?;

    let metadata = if include_metadata {
        Some(EventItemMetadata {
            name: event.name.clone(),
            fields: event
                .arguments
                .iter()
                .enumerate()
                .map(|(i, arg_ty)| EventField {
                    name: Some(format!("arg{}", i)),
                    ty: arg_ty.clone(),
                    type_name: Some(arg_ty.clone()),
                    docs: vec![],
                })
                .collect(),
            index: idx.to_string(),
            docs: event.docs.clone(),
            args: event.arguments.clone(),
        })
    } else {
        None
    };

    // Convert event name to camelCase for response
    let event_item = to_camel_case(&event.name);

    Ok((
        StatusCode::OK,
        Json(PalletEventItemResponse {
            at,
            pallet: pallet_name.to_lowercase(),
            pallet_index: pallet_index.to_string(),
            event_item,
            metadata,
        }),
    )
        .into_response())
}

// ============================================================================
// V14 Extraction (Modern Metadata with Type Registry)
// ============================================================================

fn find_pallet_v14(
    pallets: &[frame_metadata::v14::PalletMetadata<scale_info::form::PortableForm>],
    pallet_id: &str,
) -> Option<(String, u8)> {
    let pallet_id_lower = pallet_id.to_lowercase();

    // Try to parse as index first
    if let Ok(index) = pallet_id.parse::<u8>() {
        for pallet in pallets {
            if pallet.index == index {
                return Some((pallet.name.clone(), index));
            }
        }
    }

    // Otherwise search by name (case-insensitive)
    for pallet in pallets {
        if pallet.name.to_lowercase() == pallet_id_lower {
            return Some((pallet.name.clone(), pallet.index));
        }
    }

    None
}

fn extract_events_v14(
    meta: &frame_metadata::v14::RuntimeMetadataV14,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
) -> Result<Response, PalletError> {
    let (pallet_name, pallet_index) =
        find_pallet_v14(&meta.pallets, pallet_id).ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet = meta
        .pallets
        .iter()
        .find(|p| p.index == pallet_index)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    // Get event type ID
    let event_type_id = match &pallet.event {
        Some(event) => event.ty.id,
        None => {
            // No events in this pallet - return error (matching Sidecar behavior)
            return Err(PalletError::NoEventsInPallet(pallet_id.to_string()));
        }
    };

    // Resolve the event type from registry
    let event_type = meta
        .types
        .resolve(event_type_id)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    // Events are enums
    let variants = match &event_type.type_def {
        scale_info::TypeDef::Variant(v) => &v.variants,
        _ => {
            // Type exists but is not an enum variant - no queryable events
            return Err(PalletError::NoEventsInPallet(pallet_id.to_string()));
        }
    };

    let items = if only_ids {
        EventsItems::OnlyIds(variants.iter().map(|v| v.name.clone()).collect())
    } else {
        EventsItems::Full(
            variants
                .iter()
                .map(|v| {
                    let args: Vec<String> = v
                        .fields
                        .iter()
                        .map(|f| resolve_type_name_v14(&meta.types, f.ty.id))
                        .collect();
                    EventItemMetadata {
                        name: v.name.clone(),
                        fields: v
                            .fields
                            .iter()
                            .map(|f| EventField {
                                name: f.name.clone().filter(|s| !s.is_empty()),
                                ty: f.ty.id.to_string(),
                                type_name: f.type_name.clone(),
                                docs: f.docs.clone(),
                            })
                            .collect(),
                        index: v.index.to_string(),
                        docs: v.docs.clone(),
                        args,
                    }
                })
                .collect(),
        )
    };

    Ok((
        StatusCode::OK,
        Json(PalletEventsResponse {
            at,
            pallet: pallet_name.to_lowercase(),
            pallet_index: pallet_index.to_string(),
            items,
        }),
    )
        .into_response())
}

fn extract_event_item_v14(
    meta: &frame_metadata::v14::RuntimeMetadataV14,
    pallet_id: &str,
    event_item_id: &str,
    at: AtResponse,
    include_metadata: bool,
) -> Result<Response, PalletError> {
    let (pallet_name, pallet_index) =
        find_pallet_v14(&meta.pallets, pallet_id).ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet = meta
        .pallets
        .iter()
        .find(|p| p.index == pallet_index)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    // Get event type ID
    let event_type_id = match &pallet.event {
        Some(event) => event.ty.id,
        None => return Err(PalletError::EventNotFound(event_item_id.to_string())),
    };

    // Resolve the event type from registry
    let event_type = meta
        .types
        .resolve(event_type_id)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    // Events are enums
    let variants = match &event_type.type_def {
        scale_info::TypeDef::Variant(v) => &v.variants,
        _ => return Err(PalletError::EventNotFound(event_item_id.to_string())),
    };

    let event_id_lower = event_item_id.to_lowercase();
    let variant = variants
        .iter()
        .find(|v| v.name.to_lowercase() == event_id_lower)
        .ok_or_else(|| PalletError::EventNotFound(event_item_id.to_string()))?;

    let metadata = if include_metadata {
        let args: Vec<String> = variant
            .fields
            .iter()
            .map(|f| resolve_type_name_v14(&meta.types, f.ty.id))
            .collect();
        Some(EventItemMetadata {
            name: variant.name.clone(),
            fields: variant
                .fields
                .iter()
                .map(|f| EventField {
                    name: f.name.clone().filter(|s| !s.is_empty()),
                    ty: f.ty.id.to_string(),
                    type_name: f.type_name.clone(),
                    docs: f.docs.clone(),
                })
                .collect(),
            index: variant.index.to_string(),
            docs: variant.docs.clone(),
            args,
        })
    } else {
        None
    };

    // Convert event name to camelCase for response
    let event_item = to_camel_case(&variant.name);

    Ok((
        StatusCode::OK,
        Json(PalletEventItemResponse {
            at,
            pallet: pallet_name.to_lowercase(),
            pallet_index: pallet_index.to_string(),
            event_item,
            metadata,
        }),
    )
        .into_response())
}

// ============================================================================
// Type Resolution Helpers
// ============================================================================

/// Resolve a type ID to a human-readable type name for V14 metadata.
/// Matches Sidecar's format for enum types (serializes as JSON `{"_enum":[...]}`)
fn resolve_type_name_v14(types: &scale_info::PortableRegistry, type_id: u32) -> String {
    if let Some(ty) = types.resolve(type_id) {
        // Check if it's an enum (Variant) type - serialize like Sidecar does
        if let scale_info::TypeDef::Variant(v) = &ty.type_def {
            // Only simple enums (no fields) get the _enum format
            let is_simple_enum = v.variants.iter().all(|var| var.fields.is_empty());
            if is_simple_enum {
                let variant_names: Vec<String> = v
                    .variants
                    .iter()
                    .map(|var| format!("\"{}\"", var.name))
                    .collect();
                return format!("{{\"_enum\":[{}]}}", variant_names.join(","));
            }
        }

        // Use the type's path if available (e.g., "AccountId32", "u128")
        if !ty.path.segments.is_empty() {
            return ty.path.segments.last().unwrap().clone();
        }
        // Fall back to type definition for primitives
        match &ty.type_def {
            scale_info::TypeDef::Primitive(p) => format!("{:?}", p).to_lowercase(),
            scale_info::TypeDef::Compact(c) => {
                format!("Compact<{}>", resolve_type_name_v14(types, c.type_param.id))
            }
            scale_info::TypeDef::Sequence(s) => {
                // Vec<u8> is shown as "Bytes" to match Sidecar
                let inner = resolve_type_name_v14(types, s.type_param.id);
                if inner == "u8" {
                    "Bytes".to_string()
                } else {
                    format!("Vec<{}>", inner)
                }
            }
            scale_info::TypeDef::Array(a) => {
                format!("[{}; {}]", resolve_type_name_v14(types, a.type_param.id), a.len)
            }
            scale_info::TypeDef::Tuple(t) => {
                let inner: Vec<String> = t.fields.iter().map(|f| resolve_type_name_v14(types, f.id)).collect();
                format!("({})", inner.join(", "))
            }
            _ => type_id.to_string(),
        }
    } else {
        type_id.to_string()
    }
}

/// Resolve a type ID to a human-readable type name for V15 metadata.
fn resolve_type_name_v15(types: &scale_info::PortableRegistry, type_id: u32) -> String {
    // V15 uses the same PortableRegistry as V14
    resolve_type_name_v14(types, type_id)
}

// ============================================================================
// V15 Extraction
// ============================================================================

fn find_pallet_v15(
    pallets: &[frame_metadata::v15::PalletMetadata<scale_info::form::PortableForm>],
    pallet_id: &str,
) -> Option<(String, u8)> {
    let pallet_id_lower = pallet_id.to_lowercase();

    // Try to parse as index first
    if let Ok(index) = pallet_id.parse::<u8>() {
        for pallet in pallets {
            if pallet.index == index {
                return Some((pallet.name.clone(), index));
            }
        }
    }

    // Otherwise search by name (case-insensitive)
    for pallet in pallets {
        if pallet.name.to_lowercase() == pallet_id_lower {
            return Some((pallet.name.clone(), pallet.index));
        }
    }

    None
}

fn extract_events_v15(
    meta: &frame_metadata::v15::RuntimeMetadataV15,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
) -> Result<Response, PalletError> {
    let (pallet_name, pallet_index) =
        find_pallet_v15(&meta.pallets, pallet_id).ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet = meta
        .pallets
        .iter()
        .find(|p| p.index == pallet_index)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    // Get event type ID
    let event_type_id = match &pallet.event {
        Some(event) => event.ty.id,
        None => {
            // No events in this pallet - return error (matching Sidecar behavior)
            return Err(PalletError::NoEventsInPallet(pallet_id.to_string()));
        }
    };

    // Resolve the event type from registry
    let event_type = meta
        .types
        .resolve(event_type_id)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    // Events are enums
    let variants = match &event_type.type_def {
        scale_info::TypeDef::Variant(v) => &v.variants,
        _ => {
            // Type exists but is not an enum variant - no queryable events
            return Err(PalletError::NoEventsInPallet(pallet_id.to_string()));
        }
    };

    let items = if only_ids {
        EventsItems::OnlyIds(variants.iter().map(|v| v.name.clone()).collect())
    } else {
        EventsItems::Full(
            variants
                .iter()
                .map(|v| {
                    let args: Vec<String> = v
                        .fields
                        .iter()
                        .map(|f| resolve_type_name_v15(&meta.types, f.ty.id))
                        .collect();
                    EventItemMetadata {
                        name: v.name.clone(),
                        fields: v
                            .fields
                            .iter()
                            .map(|f| EventField {
                                name: f.name.clone().filter(|s| !s.is_empty()),
                                ty: f.ty.id.to_string(),
                                type_name: f.type_name.clone(),
                                docs: f.docs.clone(),
                            })
                            .collect(),
                        index: v.index.to_string(),
                        docs: v.docs.clone(),
                        args,
                    }
                })
                .collect(),
        )
    };

    Ok((
        StatusCode::OK,
        Json(PalletEventsResponse {
            at,
            pallet: pallet_name.to_lowercase(),
            pallet_index: pallet_index.to_string(),
            items,
        }),
    )
        .into_response())
}

fn extract_event_item_v15(
    meta: &frame_metadata::v15::RuntimeMetadataV15,
    pallet_id: &str,
    event_item_id: &str,
    at: AtResponse,
    include_metadata: bool,
) -> Result<Response, PalletError> {
    let (pallet_name, pallet_index) =
        find_pallet_v15(&meta.pallets, pallet_id).ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet = meta
        .pallets
        .iter()
        .find(|p| p.index == pallet_index)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    // Get event type ID
    let event_type_id = match &pallet.event {
        Some(event) => event.ty.id,
        None => return Err(PalletError::EventNotFound(event_item_id.to_string())),
    };

    // Resolve the event type from registry
    let event_type = meta
        .types
        .resolve(event_type_id)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    // Events are enums
    let variants = match &event_type.type_def {
        scale_info::TypeDef::Variant(v) => &v.variants,
        _ => return Err(PalletError::EventNotFound(event_item_id.to_string())),
    };

    let event_id_lower = event_item_id.to_lowercase();
    let variant = variants
        .iter()
        .find(|v| v.name.to_lowercase() == event_id_lower)
        .ok_or_else(|| PalletError::EventNotFound(event_item_id.to_string()))?;

    let metadata = if include_metadata {
        let args: Vec<String> = variant
            .fields
            .iter()
            .map(|f| resolve_type_name_v15(&meta.types, f.ty.id))
            .collect();
        Some(EventItemMetadata {
            name: variant.name.clone(),
            fields: variant
                .fields
                .iter()
                .map(|f| EventField {
                    name: f.name.clone().filter(|s| !s.is_empty()),
                    ty: f.ty.id.to_string(),
                    type_name: f.type_name.clone(),
                    docs: f.docs.clone(),
                })
                .collect(),
            index: variant.index.to_string(),
            docs: variant.docs.clone(),
            args,
        })
    } else {
        None
    };

    // Convert event name to camelCase for response
    let event_item = to_camel_case(&variant.name);

    Ok((
        StatusCode::OK,
        Json(PalletEventItemResponse {
            at,
            pallet: pallet_name.to_lowercase(),
            pallet_index: pallet_index.to_string(),
            event_item,
            metadata,
        }),
    )
        .into_response())
}

// ============================================================================
// Utility Functions
// ============================================================================

/// Convert PascalCase to camelCase using the heck crate.
fn to_camel_case(s: &str) -> String {
    s.to_lower_camel_case()
}
