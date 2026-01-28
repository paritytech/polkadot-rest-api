//! Handler for GET /blocks/{blockId}/para-inclusions endpoint.
//!
//! This endpoint returns parachain inclusion information for a given relay chain block.
//! It extracts CandidateIncluded events from the ParaInclusion pallet to identify
//! which parachain blocks were included in the relay chain block.

use crate::state::AppState;
use crate::utils;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use scale_value::{Composite, Value, ValueDef};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sp_runtime::traits::{BlakeTwo256, Hash as HashT};
use thiserror::Error;

use super::CommonBlockError;

// ============================================================================
// Types - exported for reuse by /rc/blocks/{blockId}/para-inclusions
// ============================================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParaInclusionsQueryParams {
    pub para_id: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParaInclusionsResponse {
    pub at: AtBlock,
    pub inclusions: Vec<ParaInclusion>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AtBlock {
    pub hash: String,
    pub height: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParaInclusion {
    pub para_id: String,
    pub para_block_number: String,
    pub para_block_hash: String,
    pub descriptor: CandidateDescriptor,
    pub commitments_hash: String,
    pub core_index: String,
    pub group_index: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateDescriptor {
    pub relay_parent: String,
    pub persisted_validation_data_hash: String,
    pub pov_hash: String,
    pub erasure_root: String,
    pub para_head: String,
    pub validation_code_hash: String,
}

#[derive(Debug, Error)]
pub enum ParaInclusionsError {
    #[error(transparent)]
    Common(#[from] CommonBlockError),

    #[error("Failed to decode event data: {0}")]
    EventDataDecodeFailed(String),

    #[error("No para inclusions found at this block")]
    NoParaInclusionsFound,

    #[error("paraId {0} does not exist in this block")]
    ParaIdNotFound(u32),
}

impl From<utils::BlockIdParseError> for ParaInclusionsError {
    fn from(err: utils::BlockIdParseError) -> Self {
        ParaInclusionsError::Common(CommonBlockError::from(err))
    }
}

impl From<utils::BlockResolveError> for ParaInclusionsError {
    fn from(err: utils::BlockResolveError) -> Self {
        ParaInclusionsError::Common(CommonBlockError::from(err))
    }
}

impl IntoResponse for ParaInclusionsError {
    fn into_response(self) -> Response {
        match self {
            ParaInclusionsError::Common(err) => err.into_response(),
            ParaInclusionsError::NoParaInclusionsFound | ParaInclusionsError::ParaIdNotFound(_) => {
                let body = Json(json!({
                    "error": self.to_string(),
                }));
                (StatusCode::BAD_REQUEST, body).into_response()
            }
            ParaInclusionsError::EventDataDecodeFailed(_) => {
                let body = Json(json!({
                    "error": self.to_string(),
                }));
                (StatusCode::INTERNAL_SERVER_ERROR, body).into_response()
            }
        }
    }
}

/// Handler for GET /blocks/{blockId}/para-inclusions
///
/// Returns parachain inclusion information for a given relay chain block.
///
/// Query Parameters:
/// - `paraId` (optional): Filter results by a specific parachain ID
pub async fn get_block_para_inclusions(
    State(state): State<AppState>,
    Path(block_id): Path<String>,
    Query(params): Query<ParaInclusionsQueryParams>,
) -> Result<Response, ParaInclusionsError> {
    let block_id_parsed = block_id.parse::<utils::BlockId>()?;
    let resolved_block = utils::resolve_block(&state, Some(block_id_parsed)).await?;

    let client_at_block = state
        .client
        .at_block(resolved_block.number)
        .await
        .map_err(|e| CommonBlockError::ClientAtBlockFailed(Box::new(e)))?;

    fetch_para_inclusions_from_client(&client_at_block, &resolved_block, params.para_id).await
}

/// Shared function to fetch para inclusions from a client at a specific block.
///
/// Used by both `/blocks/{blockId}/para-inclusions` and `/rc/blocks/{blockId}/para-inclusions`.
pub async fn fetch_para_inclusions_from_client(
    client_at_block: &subxt::client::OnlineClientAtBlock<subxt::SubstrateConfig>,
    resolved_block: &utils::ResolvedBlock,
    para_id_filter: Option<u32>,
) -> Result<Response, ParaInclusionsError> {
    let events = client_at_block
        .events()
        .fetch()
        .await
        .map_err(|e| CommonBlockError::EventsDecodeFailed(e.to_string()))?;

    let mut inclusions = extract_para_inclusions_from_events(&events)?;

    if let Some(filter_para_id) = para_id_filter {
        inclusions
            .retain(|inclusion| inclusion.para_id.parse::<u32>().ok() == Some(filter_para_id));

        if inclusions.is_empty() {
            return Err(ParaInclusionsError::ParaIdNotFound(filter_para_id));
        }
    } else if inclusions.is_empty() {
        return Err(ParaInclusionsError::NoParaInclusionsFound);
    }

    inclusions.sort_by_key(|inc| inc.para_id.parse::<u32>().unwrap_or(0));

    let response = ParaInclusionsResponse {
        at: AtBlock {
            hash: resolved_block.hash.clone(),
            height: resolved_block.number.to_string(),
        },
        inclusions,
    };

    Ok(Json(response).into_response())
}

// ============================================================================
// Extraction functions - exported for reuse by /rc/blocks/{blockId}/para-inclusions
// ============================================================================

/// Extract para inclusions from events using Subxt
///
/// Filters for CandidateIncluded events from the ParaInclusion pallet
/// and extracts the relevant data from each event.
pub fn extract_para_inclusions_from_events(
    events: &subxt::events::Events<subxt::SubstrateConfig>,
) -> Result<Vec<ParaInclusion>, ParaInclusionsError> {
    let mut inclusions = Vec::new();

    for event_result in events.iter() {
        let event = match event_result {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("Failed to decode event: {:?}", e);
                continue;
            }
        };

        let pallet_name = event.pallet_name();
        let event_name = event.event_name();

        if !pallet_name.to_lowercase().contains("parainclusion") {
            continue;
        }

        if event_name != "CandidateIncluded" {
            continue;
        }

        // Decode event fields as scale_value::Value for dynamic processing
        let event_fields: Value<()> = match event.decode_fields_unchecked_as() {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("Failed to decode CandidateIncluded event fields: {:?}", e);
                continue;
            }
        };

        // Extract inclusion data from the decoded fields
        if let Some(inclusion) = extract_inclusion_from_value(&event_fields) {
            inclusions.push(inclusion);
        }
    }

    Ok(inclusions)
}

/// Extract inclusion data from decoded event fields (Subxt 0.50 events API)
///
/// The event fields are decoded directly from the event, not wrapped in an event record.
fn extract_inclusion_from_value(event_fields: &Value<()>) -> Option<ParaInclusion> {
    // Event fields come as a Composite from decode_fields_unchecked_as
    let fields_composite = match &event_fields.value {
        ValueDef::Composite(c) => c,
        _ => return None,
    };

    extract_inclusion_from_event(fields_composite)
}

fn extract_inclusion_from_event(event_data: &Composite<()>) -> Option<ParaInclusion> {
    let values: Vec<&Value<()>> = match event_data {
        Composite::Named(fields) => fields.iter().map(|(_, v)| v).collect(),
        Composite::Unnamed(values) => values.iter().collect(),
    };

    if values.len() < 4 {
        return None;
    }

    let candidate_receipt = values.first()?;

    let head_data = values.get(1)?;

    let core_index = values.get(2)?;

    let group_index = values.get(3)?;

    let (descriptor, commitments_hash, para_id) =
        extract_candidate_receipt_data(candidate_receipt)?;

    let (para_block_number, para_block_hash) = extract_head_data(head_data)?;

    let core_index_str = extract_u32_from_value(core_index)?.to_string();
    let group_index_str = extract_u32_from_value(group_index)?.to_string();

    Some(ParaInclusion {
        para_id: para_id.to_string(),
        para_block_number: para_block_number.to_string(),
        para_block_hash,
        descriptor,
        commitments_hash,
        core_index: core_index_str,
        group_index: group_index_str,
    })
}

fn extract_candidate_receipt_data(
    candidate_receipt: &Value<()>,
) -> Option<(CandidateDescriptor, String, u32)> {
    let receipt_composite = match &candidate_receipt.value {
        ValueDef::Composite(c) => c,
        _ => return None,
    };

    let descriptor_value = get_field_from_composite(receipt_composite, &["descriptor"], Some(0))?;
    let descriptor_composite = match &descriptor_value.value {
        ValueDef::Composite(c) => c,
        _ => return None,
    };

    let para_id_value =
        get_field_from_composite(descriptor_composite, &["para_id", "paraId"], Some(0))?;
    let para_id = extract_u32_from_value(para_id_value)?;

    let relay_parent = get_field_from_composite(
        descriptor_composite,
        &["relay_parent", "relayParent"],
        Some(1),
    )
    .and_then(value_to_hex_string)?;

    let persisted_validation_data_hash = get_field_from_composite(
        descriptor_composite,
        &[
            "persisted_validation_data_hash",
            "persistedValidationDataHash",
        ],
        Some(2),
    )
    .and_then(value_to_hex_string)?;

    let pov_hash =
        get_field_from_composite(descriptor_composite, &["pov_hash", "povHash"], Some(3))
            .and_then(value_to_hex_string)?;

    let erasure_root = get_field_from_composite(
        descriptor_composite,
        &["erasure_root", "erasureRoot"],
        Some(4),
    )
    .and_then(value_to_hex_string)?;

    let para_head =
        get_field_from_composite(descriptor_composite, &["para_head", "paraHead"], Some(5))
            .and_then(value_to_hex_string)?;

    let validation_code_hash = get_field_from_composite(
        descriptor_composite,
        &["validation_code_hash", "validationCodeHash"],
        Some(6),
    )
    .and_then(value_to_hex_string)?;

    let descriptor = CandidateDescriptor {
        relay_parent,
        persisted_validation_data_hash,
        pov_hash,
        erasure_root,
        para_head,
        validation_code_hash,
    };

    let commitments_hash = get_field_from_composite(
        receipt_composite,
        &["commitments_hash", "commitmentsHash"],
        Some(1),
    )
    .and_then(value_to_hex_string)?;

    Some((descriptor, commitments_hash, para_id))
}

fn extract_head_data(head_data: &Value<()>) -> Option<(u64, String)> {
    let json = serde_json::to_value(head_data).ok()?;
    let header_bytes = utils::extract_bytes_from_json(&json)?;

    let block_number = utils::extract_block_number_from_header(&header_bytes)?;

    let block_hash = BlakeTwo256::hash(&header_bytes);
    let block_hash_hex = format!("0x{}", hex::encode(block_hash.as_ref()));

    Some((block_number, block_hash_hex))
}

fn get_field_from_composite<'a>(
    composite: &'a Composite<()>,
    field_names: &[&str],
    unnamed_index: Option<usize>,
) -> Option<&'a Value<()>> {
    match composite {
        Composite::Named(fields) => fields
            .iter()
            .find(|(name, _)| field_names.iter().any(|&n| n == *name))
            .map(|(_, v)| v),
        Composite::Unnamed(values) => unnamed_index.and_then(|idx| values.get(idx)),
    }
}

fn value_to_hex_string(value: &Value<()>) -> Option<String> {
    let json = serde_json::to_value(value).ok()?;
    let bytes = utils::extract_bytes_from_json(&json)?;
    Some(format!("0x{}", hex::encode(&bytes)))
}

fn extract_u32_from_value(value: &Value<()>) -> Option<u32> {
    let json = serde_json::to_value(value).ok()?;

    if let Some(n) = json.as_u64() {
        return u32::try_from(n).ok();
    }

    if let Some(arr) = json.as_array()
        && let Some(first) = arr.first()
        && let Some(n) = first.as_u64()
    {
        return u32::try_from(n).ok();
    }

    if let Some(obj) = json.as_object()
        && let Some(val) = obj.values().next()
        && let Some(n) = val.as_u64()
    {
        return u32::try_from(n).ok();
    }

    None
}
