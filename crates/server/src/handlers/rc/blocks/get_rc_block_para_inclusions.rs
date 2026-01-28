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
use crate::utils;
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
    #[error("Relay chain not configured. Set SAS_RELAY_CHAIN_URL to enable relay chain endpoints.")]
    RelayChainNotConfigured,

    #[error(transparent)]
    ParaInclusionsError(#[from] ParaInclusionsError),
}

impl IntoResponse for RcParaInclusionsError {
    fn into_response(self) -> Response {
        match self {
            RcParaInclusionsError::RelayChainNotConfigured => {
                let body = Json(json!({
                    "error": "Relay chain not configured. Set SAS_RELAY_CHAIN_URL to enable relay chain endpoints.",
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
pub async fn get_rc_block_para_inclusions(
    State(state): State<AppState>,
    Path(block_id): Path<String>,
    Query(params): Query<ParaInclusionsQueryParams>,
) -> Result<Response, RcParaInclusionsError> {
    let relay_client = state
        .get_relay_chain_client()
        .ok_or(RcParaInclusionsError::RelayChainNotConfigured)?;

    let relay_rpc_client = state
        .get_relay_chain_rpc_client()
        .ok_or(RcParaInclusionsError::RelayChainNotConfigured)?;

    let relay_chain_rpc = state
        .get_relay_chain_rpc()
        .ok_or(RcParaInclusionsError::RelayChainNotConfigured)?;

    let block_id_parsed = block_id
        .parse::<utils::BlockId>()
        .map_err(ParaInclusionsError::from)?;

    let resolved_block =
        utils::resolve_block_with_rpc(relay_rpc_client, relay_chain_rpc, Some(block_id_parsed))
            .await
            .map_err(ParaInclusionsError::from)?;

    let client_at_block = relay_client
        .at_block(resolved_block.number)
        .await
        .map_err(|e| {
            ParaInclusionsError::Common(CommonBlockError::ClientAtBlockFailed(Box::new(e)))
        })?;

    Ok(
        fetch_para_inclusions_from_client(&client_at_block, &resolved_block, params.para_id)
            .await?,
    )
}
