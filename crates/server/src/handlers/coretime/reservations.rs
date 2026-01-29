//! Handler for /coretime/reservations endpoint.
//!
//! Returns all reservations registered on a coretime chain (parachain with Broker pallet).
//! Each reservation includes the mask (CoreMask as hex) and task assignment info.
//!
//! Reservations represent cores that are permanently reserved for specific tasks
//! and are not available for sale in the coretime marketplace.

use crate::handlers::coretime::common::{
    AtResponse, CoreAssignment, CoretimeError, CoretimeQueryParams,
    // Shared constants
    CORE_MASK_SIZE, TASK_ID_SIZE,
    ASSIGNMENT_IDLE_VARIANT, ASSIGNMENT_POOL_VARIANT, ASSIGNMENT_TASK_VARIANT,
    // Shared functions
    has_broker_pallet, decode_compact_u32,
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

// ============================================================================
// Response Types
// ============================================================================

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

// ============================================================================
// Internal Types
// ============================================================================

/// Internal representation of a schedule item from Broker::Reservations storage.
#[derive(Debug, Clone, PartialEq)]
struct ScheduleItem {
    mask: [u8; CORE_MASK_SIZE],
    assignment: CoreAssignment,
}

// ============================================================================
// Main Handler
// ============================================================================

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

// ============================================================================
// Helper Functions
// ============================================================================

/// Fetches all reservations from Broker::Reservations storage.
///
/// Broker::Reservations is a StorageValue containing a BoundedVec<ReservationRecord>.
/// Each ReservationRecord is a BoundedVec<ScheduleItem>.
async fn fetch_reservations(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Vec<ReservationInfo>, CoretimeError> {
    let reservations_addr =
        subxt::dynamic::storage::<(), scale_value::Value>("Broker", "Reservations");

    let reservations_value = match client_at_block.storage().fetch(reservations_addr, ()).await {
        Ok(value) => value,
        Err(subxt::error::StorageError::StorageEntryNotFound { .. }) => {
            // No reservations storage entry means no reservations
            return Ok(vec![]);
        }
        Err(_) => {
            return Err(CoretimeError::StorageFetchFailed {
                pallet: "Broker",
                entry: "Reservations",
            });
        }
    };

    let raw_bytes = reservations_value.into_bytes();

    // Decode reservations - it's a Vec<Vec<ScheduleItem>>
    let reservations = decode_reservations(&raw_bytes).map_err(|e| {
        CoretimeError::StorageDecodeFailed {
            pallet: "Broker",
            entry: "Reservations",
            details: e,
        }
    })?;

    // Extract info from each reservation (first schedule item of each)
    let reservation_infos: Vec<ReservationInfo> = reservations
        .into_iter()
        .map(|items| extract_reservation_info(&items))
        .collect();

    Ok(reservation_infos)
}

/// Decodes the raw bytes of Broker::Reservations storage.
/// Returns a Vec of reservation records, where each record is a Vec<ScheduleItem>.
fn decode_reservations(bytes: &[u8]) -> Result<Vec<Vec<ScheduleItem>>, String> {
    if bytes.is_empty() {
        return Ok(vec![]);
    }

    let mut offset = 0;

    // Decode outer vec length (number of reservations)
    let (outer_len, consumed) =
        decode_compact_u32(&bytes[offset..]).ok_or("Failed to decode outer vec length")?;
    offset += consumed;

    let mut reservations = Vec::with_capacity(outer_len);

    for _ in 0..outer_len {
        if offset >= bytes.len() {
            break;
        }

        // Decode inner vec length (number of schedule items in this reservation)
        let (inner_len, consumed) =
            decode_compact_u32(&bytes[offset..]).ok_or("Failed to decode inner vec length")?;
        offset += consumed;

        let mut schedule_items = Vec::with_capacity(inner_len);

        for _ in 0..inner_len {
            if offset + CORE_MASK_SIZE > bytes.len() {
                return Err("Unexpected end of data while reading mask".to_string());
            }

            // Read CoreMask (10 bytes)
            let mut mask = [0u8; CORE_MASK_SIZE];
            mask.copy_from_slice(&bytes[offset..offset + CORE_MASK_SIZE]);
            offset += CORE_MASK_SIZE;

            if offset >= bytes.len() {
                return Err("Unexpected end of data while reading assignment".to_string());
            }

            // Read CoreAssignment enum
            let assignment_byte = bytes[offset];
            offset += 1;

            let assignment = match assignment_byte {
                ASSIGNMENT_IDLE_VARIANT => CoreAssignment::Idle,
                ASSIGNMENT_POOL_VARIANT => CoreAssignment::Pool,
                ASSIGNMENT_TASK_VARIANT => {
                    if offset + TASK_ID_SIZE > bytes.len() {
                        return Err("Unexpected end of data while reading task ID".to_string());
                    }
                    let task_bytes: [u8; 4] = bytes[offset..offset + TASK_ID_SIZE]
                        .try_into()
                        .map_err(|_| "Failed to read task ID bytes")?;
                    offset += TASK_ID_SIZE;
                    CoreAssignment::Task(u32::from_le_bytes(task_bytes))
                }
                _ => {
                    return Err(format!("Unknown assignment variant: {}", assignment_byte));
                }
            };

            schedule_items.push(ScheduleItem { mask, assignment });
        }

        reservations.push(schedule_items);
    }

    Ok(reservations)
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
        // Should use first item
        assert_eq!(result.task, "1000");
    }

    // ------------------------------------------------------------------------
    // decode_reservations tests
    // ------------------------------------------------------------------------

    #[test]
    fn test_decode_reservations_empty() {
        assert_eq!(decode_reservations(&[]).unwrap(), Vec::<Vec<ScheduleItem>>::new());
    }

    #[test]
    fn test_decode_reservations_single_reservation_idle() {
        // Encode: Vec<Vec<ScheduleItem>> with 1 reservation, 1 item (Idle)
        let mut bytes = vec![0x04]; // outer len = 1 (compact)
        bytes.push(0x04); // inner len = 1 (compact)
        bytes.extend_from_slice(&[0xFF; CORE_MASK_SIZE]); // mask
        bytes.push(ASSIGNMENT_IDLE_VARIANT); // assignment = Idle

        let result = decode_reservations(&bytes).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].len(), 1);
        assert_eq!(result[0][0].mask, [0xFF; CORE_MASK_SIZE]);
        assert_eq!(result[0][0].assignment, CoreAssignment::Idle);
    }

    #[test]
    fn test_decode_reservations_single_reservation_pool() {
        let mut bytes = vec![0x04]; // outer len = 1
        bytes.push(0x04); // inner len = 1
        bytes.extend_from_slice(&[0xAA; CORE_MASK_SIZE]); // mask
        bytes.push(ASSIGNMENT_POOL_VARIANT); // assignment = Pool

        let result = decode_reservations(&bytes).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0][0].assignment, CoreAssignment::Pool);
    }

    #[test]
    fn test_decode_reservations_single_reservation_task() {
        let mut bytes = vec![0x04]; // outer len = 1
        bytes.push(0x04); // inner len = 1
        bytes.extend_from_slice(&[0xFF; CORE_MASK_SIZE]); // mask
        bytes.push(ASSIGNMENT_TASK_VARIANT); // assignment = Task
        bytes.extend_from_slice(&1000u32.to_le_bytes()); // task ID

        let result = decode_reservations(&bytes).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0][0].assignment, CoreAssignment::Task(1000));
    }

    #[test]
    fn test_decode_reservations_multiple() {
        // Two reservations, each with 1 item
        let mut bytes = vec![0x08]; // outer len = 2 (compact)

        // First reservation: Task 1000
        bytes.push(0x04); // inner len = 1
        bytes.extend_from_slice(&[0xFF; CORE_MASK_SIZE]);
        bytes.push(ASSIGNMENT_TASK_VARIANT);
        bytes.extend_from_slice(&1000u32.to_le_bytes());

        // Second reservation: Pool
        bytes.push(0x04); // inner len = 1
        bytes.extend_from_slice(&[0xAA; CORE_MASK_SIZE]);
        bytes.push(ASSIGNMENT_POOL_VARIANT);

        let result = decode_reservations(&bytes).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0][0].assignment, CoreAssignment::Task(1000));
        assert_eq!(result[1][0].assignment, CoreAssignment::Pool);
    }

    #[test]
    fn test_decode_reservations_invalid_assignment_variant() {
        let mut bytes = vec![0x04]; // outer len = 1
        bytes.push(0x04); // inner len = 1
        bytes.extend_from_slice(&[0xFF; CORE_MASK_SIZE]);
        bytes.push(99); // Invalid assignment variant

        let result = decode_reservations(&bytes);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Unknown assignment variant"));
    }

    #[test]
    fn test_decode_reservations_truncated_mask() {
        let mut bytes = vec![0x04]; // outer len = 1
        bytes.push(0x04); // inner len = 1
        bytes.extend_from_slice(&[0xFF; 5]); // Only 5 bytes of mask (should be 10)

        let result = decode_reservations(&bytes);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_reservations_truncated_task_id() {
        let mut bytes = vec![0x04]; // outer len = 1
        bytes.push(0x04); // inner len = 1
        bytes.extend_from_slice(&[0xFF; CORE_MASK_SIZE]);
        bytes.push(ASSIGNMENT_TASK_VARIANT);
        bytes.extend_from_slice(&[0x00, 0x01]); // Only 2 bytes of task ID (should be 4)

        let result = decode_reservations(&bytes);
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------------
    // Response type tests
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
            reservations: vec![
                ReservationInfo {
                    mask: "0xffffffffffffffffffff".to_string(),
                    task: "1000".to_string(),
                },
            ],
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"at\""));
        assert!(json.contains("\"reservations\""));
        assert!(json.contains("\"hash\":\"0xabc123\""));
        assert!(json.contains("\"height\":\"12345\""));
    }
}
