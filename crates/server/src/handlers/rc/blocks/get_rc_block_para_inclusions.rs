// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Handler for GET /rc/blocks/{blockId}/para-inclusions endpoint.
//!
//! This endpoint returns parachain inclusion information for a given relay chain block.
//! It extracts CandidateIncluded events from the ParaInclusion pallet to identify
//! which parachain blocks were included in the relay chain block.

use crate::handlers::blocks::{
    CommonBlockError, ParaInclusionsError, ParaInclusionsQueryParams,
    fetch_para_inclusions_from_client,
};
use crate::state::AppState;
use crate::utils::{self, BlockId, ResolvedBlock};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RcParaInclusionsError {
    #[error(
        "Relay chain not configured. Set SAS_SUBSTRATE_MULTI_CHAIN_URL to enable relay chain endpoints."
    )]
    RelayChainNotConfigured,

    #[error(transparent)]
    ParaInclusionsError(#[from] ParaInclusionsError),
}

impl IntoResponse for RcParaInclusionsError {
    fn into_response(self) -> Response {
        match self {
            RcParaInclusionsError::RelayChainNotConfigured => {
                let body = Json(json!({
                    "error": "Relay chain not configured. Set SAS_SUBSTRATE_MULTI_CHAIN_URL to enable relay chain endpoints.",
                }));
                (StatusCode::SERVICE_UNAVAILABLE, body).into_response()
            }
            RcParaInclusionsError::ParaInclusionsError(inner) => inner.into_response(),
        }
    }
}

/// Handler for GET /rc/blocks/{blockId}/para-inclusions
///
/// Returns parachain inclusion information for a given relay chain block.
///
/// Query Parameters:
/// - `paraId` (optional): Filter results by a specific parachain ID
#[utoipa::path(
    get,
    path = "/v1/rc/blocks/{blockId}/para-inclusions",
    tag = "rc",
    summary = "RC get parachain inclusions",
    description = "Returns parachain inclusion information for a given relay chain block.",
    params(
        ("blockId" = String, Path, description = "Block height number or block hash"),
        ("paraId" = Option<u32>, Query, description = "Filter by parachain ID")
    ),
    responses(
        (status = 200, description = "Parachain inclusions", body = Object),
        (status = 400, description = "Invalid block identifier"),
        (status = 503, description = "Relay chain not configured"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_rc_block_para_inclusions(
    State(state): State<AppState>,
    Path(block_id): Path<String>,
    Query(params): Query<ParaInclusionsQueryParams>,
) -> Result<Response, RcParaInclusionsError> {
    let relay_client = state
        .get_relay_chain_client()
        .ok_or(RcParaInclusionsError::RelayChainNotConfigured)?;

    let block_id_parsed = block_id
        .parse::<utils::BlockId>()
        .map_err(ParaInclusionsError::from)?;

    let client_at_block = match block_id_parsed {
        BlockId::Number(number) => relay_client.at_block(number).await,
        BlockId::Hash(hash) => relay_client.at_block(hash).await,
    }
    .map_err(|e| ParaInclusionsError::Common(CommonBlockError::ClientAtBlockFailed(Box::new(e))))?;

    let resolved_block = ResolvedBlock {
        hash: format!("{:#x}", client_at_block.block_hash()),
        number: client_at_block.block_number(),
    };

    Ok(
        fetch_para_inclusions_from_client(&client_at_block, &resolved_block, params.para_id)
            .await?,
    )
}
