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
use parity_scale_codec::Decode;
use scale_value::{Composite, Value, ValueDef};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sp_runtime::traits::{BlakeTwo256, Hash as HashT};
use subxt_historic::error::OnlineClientAtBlockError;
use thiserror::Error;

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
    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] utils::BlockIdParseError),

    #[error("Block resolution failed")]
    BlockResolveFailed(#[from] utils::BlockResolveError),

    #[error("Failed to get client at block: {0}")]
    ClientAtBlockFailed(#[from] OnlineClientAtBlockError),

    #[error("Failed to fetch storage: {0}")]
    StorageFetchFailed(String),

    #[error("Failed to decode events")]
    EventsDecodeFailed(#[source] subxt_historic::error::StorageValueError),

    #[error("Failed to decode event data: {0}")]
    EventDataDecodeFailed(String),

    #[error("No para inclusions found at this block")]
    NoParaInclusionsFound,
}

impl IntoResponse for ParaInclusionsError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            ParaInclusionsError::InvalidBlockParam(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            ParaInclusionsError::BlockResolveFailed(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            ParaInclusionsError::NoParaInclusionsFound => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            ParaInclusionsError::ClientAtBlockFailed(err) => {
                if crate::utils::is_online_client_at_block_disconnected(err) {
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        format!("Service temporarily unavailable: {}", err),
                    )
                } else {
                    (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
                }
            }
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
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

    let client_at_block = state.client.at(resolved_block.number).await?;

    let storage_entry = client_at_block
        .storage()
        .entry("System", "Events")
        .map_err(|e| ParaInclusionsError::StorageFetchFailed(e.to_string()))?;

    let events_value = storage_entry
        .fetch(())
        .await
        .map_err(|e| ParaInclusionsError::StorageFetchFailed(e.to_string()))?
        .ok_or_else(|| {
            ParaInclusionsError::StorageFetchFailed("Events storage not found".to_string())
        })?;

    let events_decoded: Value<()> = events_value
        .decode_as()
        .map_err(ParaInclusionsError::EventsDecodeFailed)?;

    let mut inclusions = extract_para_inclusions(&events_decoded)?;

    if let Some(filter_para_id) = params.para_id {
        inclusions
            .retain(|inclusion| inclusion.para_id.parse::<u32>().ok() == Some(filter_para_id));
    }

    inclusions.sort_by_key(|inc| inc.para_id.parse::<u32>().unwrap_or(0));

    if inclusions.is_empty() {
        return Err(ParaInclusionsError::NoParaInclusionsFound);
    }

    let response = ParaInclusionsResponse {
        at: AtBlock {
            hash: resolved_block.hash.clone(),
            height: resolved_block.number.to_string(),
        },
        inclusions,
    };

    Ok(Json(response).into_response())
}

fn extract_para_inclusions(
    events_decoded: &Value<()>,
) -> Result<Vec<ParaInclusion>, ParaInclusionsError> {
    let mut inclusions = Vec::new();

    let events_composite = match &events_decoded.value {
        ValueDef::Composite(composite) => composite,
        _ => return Ok(inclusions),
    };

    let events_values = match events_composite {
        Composite::Unnamed(values) => values,
        Composite::Named(_) => return Ok(inclusions),
    };

    for event_record in events_values.iter() {
        let record_composite = match &event_record.value {
            ValueDef::Composite(c) => c,
            _ => continue,
        };

        let event_value = match record_composite {
            Composite::Named(fields) => fields
                .iter()
                .find(|(name, _)| name == "event")
                .map(|(_, v)| v),
            Composite::Unnamed(values) => values.get(1),
        };

        let event = match event_value {
            Some(v) => v,
            None => continue,
        };

        let event_variant = match &event.value {
            ValueDef::Variant(variant) => variant,
            _ => continue,
        };

        let pallet_name = &event_variant.name;
        if !pallet_name.to_lowercase().contains("parainclusion") {
            continue;
        }

        let (event_name, event_data) = match &event_variant.values {
            Composite::Unnamed(values) => {
                let first_val = match values.first() {
                    Some(v) => v,
                    None => continue,
                };
                match &first_val.value {
                    ValueDef::Variant(inner_variant) => {
                        (inner_variant.name.clone(), &inner_variant.values)
                    }
                    _ => continue,
                }
            }
            Composite::Named(fields) => {
                let (_name, val) = match fields.first() {
                    Some((n, v)) => (n, v),
                    None => continue,
                };
                match &val.value {
                    ValueDef::Variant(inner_variant) => {
                        (inner_variant.name.clone(), &inner_variant.values)
                    }
                    _ => continue,
                }
            }
        };

        if event_name != "CandidateIncluded" {
            continue;
        }

        if let Some(inclusion) = extract_inclusion_from_event(event_data) {
            inclusions.push(inclusion);
        }
    }

    Ok(inclusions)
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
    let header_bytes = extract_bytes_from_json(&json)?;

    let block_number = extract_block_number_from_header(&header_bytes)?;

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
    let bytes = extract_bytes_from_json(&json)?;
    Some(format!("0x{}", hex::encode(&bytes)))
}

fn extract_u32_from_value(value: &Value<()>) -> Option<u32> {
    let json = serde_json::to_value(value).ok()?;

    if let Some(n) = json.as_u64() {
        return u32::try_from(n).ok();
    }

    if let Some(arr) = json.as_array() {
        if let Some(first) = arr.first() {
            if let Some(n) = first.as_u64() {
                return u32::try_from(n).ok();
            }
        }
    }

    if let Some(obj) = json.as_object() {
        if let Some(val) = obj.values().next() {
            if let Some(n) = val.as_u64() {
                return u32::try_from(n).ok();
            }
        }
    }

    None
}

fn extract_bytes_from_json(json: &serde_json::Value) -> Option<Vec<u8>> {
    match json {
        serde_json::Value::Array(arr) => {
            let bytes: Vec<u8> = arr
                .iter()
                .filter_map(|v| v.as_u64().and_then(|n| (n <= 255).then_some(n as u8)))
                .collect();

            if !bytes.is_empty() {
                return Some(bytes);
            }

            if arr.len() == 1 {
                return extract_bytes_from_json(&arr[0]);
            }

            None
        }
        serde_json::Value::String(s) => {
            let hex_clean = s.strip_prefix("0x").unwrap_or(s);
            hex::decode(hex_clean).ok()
        }
        _ => None,
    }
}

fn extract_block_number_from_header(header_bytes: &[u8]) -> Option<u64> {
    if header_bytes.len() < 32 {
        return None;
    }

    let mut cursor = &header_bytes[32..];
    let number_compact = parity_scale_codec::Compact::<u32>::decode(&mut cursor).ok()?;
    Some(number_compact.0 as u64)
}
