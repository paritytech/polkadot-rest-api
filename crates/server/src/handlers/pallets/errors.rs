//! Handler for `/pallets/{palletId}/errors` endpoint.

use crate::handlers::pallets::common::{
    AtResponse, FieldMetadata, PalletError, PalletQueryParams, RcBlockFields, find_pallet_v14,
    find_pallet_v15,
};
use crate::state::AppState;
use crate::utils;
use crate::utils::rc_block::find_ah_blocks_in_rc_block;
use axum::{
    Json, extract::Path, extract::Query, extract::State, http::StatusCode, response::IntoResponse,
    response::Response,
};
use config::ChainType;
use frame_metadata::RuntimeMetadata;
use parity_scale_codec::Decode;
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PalletsErrorsResponse {
    pub at: AtResponse,
    pub pallet: String,
    pub pallet_index: String,
    pub items: ErrorsItems,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ErrorsItems {
    Full(Vec<ErrorItemMetadata>),
    OnlyIds(Vec<String>),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorItemMetadata {
    pub name: String,
    pub fields: Vec<FieldMetadata>,
    pub index: String,
    pub docs: Vec<String>,
    pub args: Vec<String>,
}

pub async fn get_pallets_errors(
    State(state): State<AppState>,
    Path(pallet_id): Path<String>,
    Query(params): Query<PalletQueryParams>,
) -> Result<Response, PalletError> {
    if params.use_rc_block {
        return handle_use_rc_block(state, pallet_id, params).await;
    }

    let block_id = params
        .at
        .map(|s| s.parse::<crate::utils::BlockId>())
        .transpose()?;

    let resolved_block = utils::resolve_block(&state, block_id).await?;
    let client_at_block = state.client.at(resolved_block.number).await?;
    let metadata = client_at_block.metadata();

    let at = AtResponse {
        hash: resolved_block.hash.clone(),
        height: resolved_block.number.to_string(),
    };

    let response = extract_errors_from_metadata(
        metadata,
        &pallet_id,
        at,
        params.only_ids,
        RcBlockFields::default(),
    )?;

    Ok((StatusCode::OK, Json(response)).into_response())
}

async fn handle_use_rc_block(
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
        let at = AtResponse {
            hash: rc_resolved_block.hash.clone(),
            height: rc_resolved_block.number.to_string(),
        };
        return Ok((
            StatusCode::OK,
            Json(PalletsErrorsResponse {
                at,
                pallet: pallet_id.to_lowercase(),
                pallet_index: "0".to_string(),
                items: ErrorsItems::Full(vec![]),
                rc_block_hash: Some(rc_resolved_block.hash),
                rc_block_number: Some(rc_resolved_block.number.to_string()),
                ah_timestamp: None,
            }),
        )
            .into_response());
    }

    let ah_block = &ah_blocks[0];
    let client_at_block = state.client.at(ah_block.number).await?;
    let metadata = client_at_block.metadata();

    let at = AtResponse {
        hash: ah_block.hash.clone(),
        height: ah_block.number.to_string(),
    };

    let mut ah_timestamp = None;
    if let Ok(timestamp_entry) = client_at_block.storage().entry("Timestamp", "Now")
        && let Ok(Some(timestamp)) = timestamp_entry.fetch(()).await
    {
        let timestamp_bytes = timestamp.into_bytes();
        let mut cursor = &timestamp_bytes[..];
        if let Ok(timestamp_value) = u64::decode(&mut cursor) {
            ah_timestamp = Some(timestamp_value.to_string());
        }
    }

    let rc_fields = RcBlockFields {
        rc_block_hash: Some(rc_resolved_block.hash),
        rc_block_number: Some(rc_resolved_block.number.to_string()),
        ah_timestamp,
    };

    let response =
        extract_errors_from_metadata(metadata, &pallet_id, at, params.only_ids, rc_fields)?;

    Ok((StatusCode::OK, Json(response)).into_response())
}

fn extract_errors_from_metadata(
    metadata: &RuntimeMetadata,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsErrorsResponse, PalletError> {
    match metadata {
        RuntimeMetadata::V14(meta) => extract_errors_v14(meta, pallet_id, at, only_ids, rc_fields),
        RuntimeMetadata::V15(meta) => extract_errors_v15(meta, pallet_id, at, only_ids, rc_fields),
        _ => Err(PalletError::UnsupportedMetadataVersion),
    }
}

fn extract_errors_v14(
    meta: &frame_metadata::v14::RuntimeMetadataV14,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsErrorsResponse, PalletError> {
    let (pallet_name, pallet_index) = find_pallet_v14(&meta.pallets, pallet_id)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet = meta
        .pallets
        .iter()
        .find(|p| p.index == pallet_index)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let items = if let Some(error) = &pallet.error {
        extract_error_variants(&meta.types, error.ty.id, only_ids)
    } else if only_ids {
        ErrorsItems::OnlyIds(vec![])
    } else {
        ErrorsItems::Full(vec![])
    };

    Ok(PalletsErrorsResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: pallet_index.to_string(),
        items,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}

fn extract_error_variants(
    types: &scale_info::PortableRegistry,
    type_id: u32,
    only_ids: bool,
) -> ErrorsItems {
    let Some(ty) = types.resolve(type_id) else {
        return if only_ids {
            ErrorsItems::OnlyIds(vec![])
        } else {
            ErrorsItems::Full(vec![])
        };
    };

    let scale_info::TypeDef::Variant(variant_def) = &ty.type_def else {
        return if only_ids {
            ErrorsItems::OnlyIds(vec![])
        } else {
            ErrorsItems::Full(vec![])
        };
    };

    if only_ids {
        ErrorsItems::OnlyIds(
            variant_def
                .variants
                .iter()
                .map(|v| v.name.clone())
                .collect(),
        )
    } else {
        ErrorsItems::Full(
            variant_def
                .variants
                .iter()
                .map(|v| {
                    let fields: Vec<FieldMetadata> = v
                        .fields
                        .iter()
                        .map(|f| FieldMetadata {
                            name: f.name.clone().unwrap_or_default(),
                            ty: f.ty.id.to_string(),
                            type_name: f.type_name.clone().unwrap_or_default(),
                            docs: f.docs.clone(),
                        })
                        .collect();

                    let args: Vec<String> = v
                        .fields
                        .iter()
                        .map(|f| f.type_name.clone().unwrap_or_default())
                        .collect();

                    ErrorItemMetadata {
                        name: v.name.clone(),
                        fields,
                        index: v.index.to_string(),
                        docs: v.docs.clone(),
                        args,
                    }
                })
                .collect(),
        )
    }
}

fn extract_errors_v15(
    meta: &frame_metadata::v15::RuntimeMetadataV15,
    pallet_id: &str,
    at: AtResponse,
    only_ids: bool,
    rc_fields: RcBlockFields,
) -> Result<PalletsErrorsResponse, PalletError> {
    let (pallet_name, pallet_index) = find_pallet_v15(&meta.pallets, pallet_id)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let pallet = meta
        .pallets
        .iter()
        .find(|p| p.index == pallet_index)
        .ok_or_else(|| PalletError::PalletNotFound(pallet_id.to_string()))?;

    let items = if let Some(error) = &pallet.error {
        extract_error_variants(&meta.types, error.ty.id, only_ids)
    } else if only_ids {
        ErrorsItems::OnlyIds(vec![])
    } else {
        ErrorsItems::Full(vec![])
    };

    Ok(PalletsErrorsResponse {
        at,
        pallet: pallet_name.to_lowercase(),
        pallet_index: pallet_index.to_string(),
        items,
        rc_block_hash: rc_fields.rc_block_hash,
        rc_block_number: rc_fields.rc_block_number,
        ah_timestamp: rc_fields.ah_timestamp,
    })
}
