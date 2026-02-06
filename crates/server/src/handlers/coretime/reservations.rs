use crate::handlers::coretime::common::{
    AtResponse, CoretimeError, CoretimeQueryParams, ScheduleItem, has_broker_pallet,
};
use crate::state::AppState;
use crate::utils::{BlockId, resolve_block};
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use primitive_types::H256;
use serde::Serialize;
use std::str::FromStr;
use subxt::{SubstrateConfig, client::OnlineClientAtBlock};

/// Information about a single reservation.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReservationInfo {
    /// The CoreMask as a hex string (0x-prefixed).
    pub mask: String,
    /// The task assignment: task ID as string, "Pool", or empty string for Idle.
    pub task: String,
}

/// Response for GET /coretime/reservations endpoint.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoretimeReservationsResponse {
    /// Block context (hash and height).
    pub at: AtResponse,
    /// List of reservations with their mask and task info.
    pub reservations: Vec<ReservationInfo>,
}

/// Handler for GET /coretime/reservations endpoint.
///
/// Returns all reservations registered on a coretime chain. Each reservation includes:
/// - mask: The CoreMask as a hex string
/// - task: The task assignment (task ID, "Pool", or empty for Idle)
///
/// Reservations are cores that are permanently reserved and not available for sale.
///
/// Query Parameters:
/// - at: Optional block number or hash to query at (defaults to latest finalized)
pub async fn coretime_reservations(
    State(state): State<AppState>,
    Query(params): Query<CoretimeQueryParams>,
) -> Result<Response, CoretimeError> {
    // Parse the block ID if provided
    let block_id = match &params.at {
        None => None,
        Some(at_str) => Some(at_str.parse::<BlockId>()?),
    };

    // Resolve the block first to get a proper "Block not found" error
    // if the block doesn't exist (instead of a generic client error)
    let resolved_block = resolve_block(&state, block_id).await?;

    // Get client at the resolved block hash
    let block_hash =
        H256::from_str(&resolved_block.hash).map_err(|_| CoretimeError::InvalidBlockHash)?;
    let client_at_block = state.client.at_block(block_hash).await?;

    let at = AtResponse {
        hash: resolved_block.hash,
        height: resolved_block.number.to_string(),
    };

    // Verify that the Broker pallet exists at this block
    if !has_broker_pallet(&client_at_block) {
        return Err(CoretimeError::BrokerPalletNotFound);
    }

    // Fetch reservations
    let reservations = fetch_reservations(&client_at_block).await?;

    Ok((
        StatusCode::OK,
        Json(CoretimeReservationsResponse { at, reservations }),
    )
        .into_response())
}

pub async fn fetch_reservations(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<ReservationInfo>, CoretimeError> {
    let reservations_addr = subxt::dynamic::storage::<(), ()>("Broker", "Reservations");

    let reservations_value = match client_at_block.storage().fetch(reservations_addr, ()).await {
        Ok(value) => value,
        Err(subxt::error::StorageError::StorageEntryNotFound { .. }) => {
            return Ok(vec![]);
        }
        Err(_) => {
            return Err(CoretimeError::StorageFetchFailed {
                pallet: "Broker",
                entry: "Reservations",
            });
        }
    };

    // Decode directly into typed Vec<Vec<ScheduleItem>>
    let reservations: Vec<Vec<ScheduleItem>> =
        reservations_value
            .decode_as()
            .map_err(|e| CoretimeError::StorageDecodeFailed {
                pallet: "Broker",
                entry: "Reservations",
                details: e.to_string(),
            })?;

    Ok(reservations
        .iter()
        .map(|items| extract_reservation_info(items))
        .collect())
}

/// Extracts reservation info from a list of schedule items.
/// Uses the first schedule item's mask and assignment.
fn extract_reservation_info(items: &[ScheduleItem]) -> ReservationInfo {
    if items.is_empty() {
        return ReservationInfo {
            mask: String::new(),
            task: String::new(),
        };
    }

    let first = &items[0];
    let mask = format!("0x{}", hex::encode(first.mask));
    let task = first.assignment.to_task_string();

    ReservationInfo { mask, task }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::coretime::common::{CORE_MASK_SIZE, CoreAssignment, ScheduleItem};

    // ------------------------------------------------------------------------
    // extract_reservation_info tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_extract_reservation_info_empty() {
        let result = extract_reservation_info(&[]);
        assert_eq!(result.mask, "");
        assert_eq!(result.task, "");
    }

    #[test]
    fn test_extract_reservation_info_idle() {
        let items = vec![ScheduleItem {
            mask: [0xFF; CORE_MASK_SIZE],
            assignment: CoreAssignment::Idle,
        }];

        let result = extract_reservation_info(&items);
        assert_eq!(result.mask, "0xffffffffffffffffffff");
        assert_eq!(result.task, "");
    }

    #[test]
    fn test_extract_reservation_info_pool() {
        let items = vec![ScheduleItem {
            mask: [0xAA; CORE_MASK_SIZE],
            assignment: CoreAssignment::Pool,
        }];

        let result = extract_reservation_info(&items);
        assert_eq!(result.mask, "0xaaaaaaaaaaaaaaaaaaaa");
        assert_eq!(result.task, "Pool");
    }

    #[test]
    fn test_extract_reservation_info_task() {
        let items = vec![ScheduleItem {
            mask: [0xFF; CORE_MASK_SIZE],
            assignment: CoreAssignment::Task(1000),
        }];

        let result = extract_reservation_info(&items);
        assert_eq!(result.mask, "0xffffffffffffffffffff");
        assert_eq!(result.task, "1000");
    }

    #[test]
    fn test_extract_reservation_info_uses_first_item() {
        let items = vec![
            ScheduleItem {
                mask: [0xFF; CORE_MASK_SIZE],
                assignment: CoreAssignment::Task(1000),
            },
            ScheduleItem {
                mask: [0x00; CORE_MASK_SIZE],
                assignment: CoreAssignment::Task(2000),
            },
        ];

        let result = extract_reservation_info(&items);
        assert_eq!(result.task, "1000");
    }

    #[test]
    fn test_extract_reservation_info_multiple_reservations() {
        let reservations = vec![
            vec![ScheduleItem {
                mask: [0xFF; CORE_MASK_SIZE],
                assignment: CoreAssignment::Task(1000),
            }],
            vec![ScheduleItem {
                mask: [0xAA; CORE_MASK_SIZE],
                assignment: CoreAssignment::Pool,
            }],
        ];

        let result: Vec<ReservationInfo> = reservations
            .iter()
            .map(|items| extract_reservation_info(items))
            .collect();

        assert_eq!(result.len(), 2);
        assert_eq!(result[0].task, "1000");
        assert_eq!(result[1].task, "Pool");
    }

    // ------------------------------------------------------------------------
    // Serialization tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_reservation_info_serialization() {
        let info = ReservationInfo {
            mask: "0xffffffffffffffffffff".to_string(),
            task: "1000".to_string(),
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"mask\":\"0xffffffffffffffffffff\""));
        assert!(json.contains("\"task\":\"1000\""));
    }

    #[test]
    fn test_reservation_info_equality() {
        let a = ReservationInfo {
            mask: "0xff".to_string(),
            task: "100".to_string(),
        };
        let b = ReservationInfo {
            mask: "0xff".to_string(),
            task: "100".to_string(),
        };
        assert_eq!(a, b);
    }

    #[test]
    fn test_reservations_response_serialization() {
        let response = CoretimeReservationsResponse {
            at: AtResponse {
                hash: "0xabc123".to_string(),
                height: "12345".to_string(),
            },
            reservations: vec![ReservationInfo {
                mask: "0xffffffffffffffffffff".to_string(),
                task: "1000".to_string(),
            }],
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"at\""));
        assert!(json.contains("\"reservations\""));
        assert!(json.contains("\"hash\":\"0xabc123\""));
        assert!(json.contains("\"height\":\"12345\""));
    }
}
