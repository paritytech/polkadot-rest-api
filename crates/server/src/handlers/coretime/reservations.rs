use crate::handlers::coretime::common::{
    AtResponse, CoreAssignment, CoretimeError, CoretimeQueryParams, has_broker_pallet,
};
use crate::state::AppState;
use crate::utils::{BlockId, resolve_block};
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use parity_scale_codec::Decode;
use primitive_types::H256;
use scale_value::{Composite, ValueDef};
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

/// Internal representation of a schedule item extracted from scale_value.
#[derive(Debug, Clone, PartialEq)]
struct ScheduleItem {
    mask: Vec<u8>,
    assignment: CoreAssignment,
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

    // Use decode_as() to get the value as scale_value::Value
    let decoded: scale_value::Value<()> =
        reservations_value
            .decode_as()
            .map_err(|e| CoretimeError::StorageDecodeFailed {
                pallet: "Broker",
                entry: "Reservations",
                details: e.to_string(),
            })?;

    // Parse the decoded value into reservation infos
    let reservation_infos = decode_reservations_from_value(&decoded);

    Ok(reservation_infos)
}

/// Decodes reservations from a scale_value::Value.
/// The structure is Vec<Vec<ScheduleItem>> where each ScheduleItem has mask and assignment.
fn decode_reservations_from_value(value: &scale_value::Value<()>) -> Vec<ReservationInfo> {
    // Outer vec: list of reservation records
    let outer_items = match &value.value {
        ValueDef::Composite(Composite::Unnamed(items)) => items,
        _ => return vec![],
    };

    outer_items
        .iter()
        .map(|inner_value| {
            // Inner vec: list of schedule items for one reservation
            let schedule_items = parse_schedule_items(inner_value);
            extract_reservation_info(&schedule_items)
        })
        .collect()
}

/// Parses a Vec<ScheduleItem> from a scale_value::Value.
fn parse_schedule_items(value: &scale_value::Value<()>) -> Vec<ScheduleItem> {
    let items = match &value.value {
        ValueDef::Composite(Composite::Unnamed(items)) => items,
        _ => return vec![],
    };

    items.iter().filter_map(parse_schedule_item).collect()
}

/// Parses a single ScheduleItem from a scale_value::Value.
fn parse_schedule_item(value: &scale_value::Value<()>) -> Option<ScheduleItem> {
    let (mask_value, assignment_value) = match &value.value {
        ValueDef::Composite(Composite::Named(fields)) => {
            let mask = fields
                .iter()
                .find(|(name, _)| name == "mask")
                .map(|(_, v)| v)?;
            let assignment = fields
                .iter()
                .find(|(name, _)| name == "assignment")
                .map(|(_, v)| v)?;
            (mask, assignment)
        }
        ValueDef::Composite(Composite::Unnamed(fields)) if fields.len() >= 2 => {
            (&fields[0], &fields[1])
        }
        _ => return None,
    };

    // Parse mask as array of bytes
    let mask = parse_mask(mask_value);

    // Parse assignment enum
    let assignment = parse_assignment(assignment_value);

    Some(ScheduleItem { mask, assignment })
}

/// Parses a CoreMask from a scale_value::Value (array of bytes).
fn parse_mask(value: &scale_value::Value<()>) -> Vec<u8> {
    match &value.value {
        ValueDef::Composite(Composite::Unnamed(bytes)) => bytes
            .iter()
            .filter_map(|b| {
                if let ValueDef::Primitive(scale_value::Primitive::U128(n)) = &b.value {
                    Some(*n as u8)
                } else {
                    None
                }
            })
            .collect(),
        _ => vec![],
    }
}

/// Parses a CoreAssignment from a scale_value::Value (enum variant).
fn parse_assignment(value: &scale_value::Value<()>) -> CoreAssignment {
    match &value.value {
        ValueDef::Variant(variant) => match variant.name.as_str() {
            "Idle" => CoreAssignment::Idle,
            "Pool" => CoreAssignment::Pool,
            "Task" => {
                // Extract task ID from variant values
                let task_id = match &variant.values {
                    Composite::Unnamed(vals) if !vals.is_empty() => {
                        extract_u32_from_value(&vals[0]).unwrap_or(0)
                    }
                    Composite::Named(vals) if !vals.is_empty() => {
                        extract_u32_from_value(&vals[0].1).unwrap_or(0)
                    }
                    _ => 0,
                };
                CoreAssignment::Task(task_id)
            }
            _ => CoreAssignment::Idle,
        },
        _ => CoreAssignment::Idle,
    }
}

/// Extract u32 from a scale_value::Value
fn extract_u32_from_value(value: &scale_value::Value<()>) -> Option<u32> {
    match &value.value {
        ValueDef::Primitive(scale_value::Primitive::U128(n)) => Some(*n as u32),
        _ => None,
    }
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
    let mask = format!("0x{}", hex::encode(&first.mask));
    let task = first.assignment.to_task_string();

    ReservationInfo { mask, task }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scale_value::Value;

    // Helper to create a scale_value representing a ScheduleItem
    fn make_schedule_item_value(
        mask_bytes: &[u8],
        assignment: &str,
        task_id: Option<u32>,
    ) -> Value<()> {
        let mask_values: Vec<Value<()>> =
            mask_bytes.iter().map(|&b| Value::u128(b as u128)).collect();

        let assignment_value = match assignment {
            "Idle" => Value::named_variant("Idle", Vec::<(&str, Value<()>)>::new()),
            "Pool" => Value::named_variant("Pool", Vec::<(&str, Value<()>)>::new()),
            "Task" => Value::named_variant(
                "Task",
                vec![("0", Value::u128(task_id.unwrap_or(0) as u128))],
            ),
            _ => Value::named_variant("Idle", Vec::<(&str, Value<()>)>::new()),
        };

        Value::named_composite([
            ("mask", Value::unnamed_composite(mask_values)),
            ("assignment", assignment_value),
        ])
    }

    #[test]
    fn test_extract_reservation_info_empty() {
        let result = extract_reservation_info(&[]);
        assert_eq!(result.mask, "");
        assert_eq!(result.task, "");
    }

    #[test]
    fn test_extract_reservation_info_idle() {
        let items = vec![ScheduleItem {
            mask: vec![0xFF; 10],
            assignment: CoreAssignment::Idle,
        }];

        let result = extract_reservation_info(&items);
        assert_eq!(result.mask, "0xffffffffffffffffffff");
        assert_eq!(result.task, "");
    }

    #[test]
    fn test_extract_reservation_info_pool() {
        let items = vec![ScheduleItem {
            mask: vec![0xAA; 10],
            assignment: CoreAssignment::Pool,
        }];

        let result = extract_reservation_info(&items);
        assert_eq!(result.mask, "0xaaaaaaaaaaaaaaaaaaaa");
        assert_eq!(result.task, "Pool");
    }

    #[test]
    fn test_extract_reservation_info_task() {
        let items = vec![ScheduleItem {
            mask: vec![0xFF; 10],
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
                mask: vec![0xFF; 10],
                assignment: CoreAssignment::Task(1000),
            },
            ScheduleItem {
                mask: vec![0x00; 10],
                assignment: CoreAssignment::Task(2000),
            },
        ];

        let result = extract_reservation_info(&items);
        // Should use first item
        assert_eq!(result.task, "1000");
    }

    #[test]
    fn test_decode_reservations_empty() {
        let value = Value::unnamed_composite(Vec::<Value<()>>::new());
        let result = decode_reservations_from_value(&value);
        assert!(result.is_empty());
    }

    #[test]
    fn test_decode_reservations_single_reservation_idle() {
        let mask = vec![0xFF; 10];
        let schedule_item = make_schedule_item_value(&mask, "Idle", None);
        let inner_vec = Value::unnamed_composite(vec![schedule_item]);
        let outer_vec = Value::unnamed_composite(vec![inner_vec]);

        let result = decode_reservations_from_value(&outer_vec);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].mask, "0xffffffffffffffffffff");
        assert_eq!(result[0].task, "");
    }

    #[test]
    fn test_decode_reservations_single_reservation_pool() {
        let mask = vec![0xAA; 10];
        let schedule_item = make_schedule_item_value(&mask, "Pool", None);
        let inner_vec = Value::unnamed_composite(vec![schedule_item]);
        let outer_vec = Value::unnamed_composite(vec![inner_vec]);

        let result = decode_reservations_from_value(&outer_vec);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].task, "Pool");
    }

    #[test]
    fn test_decode_reservations_single_reservation_task() {
        let mask = vec![0xFF; 10];
        let schedule_item = make_schedule_item_value(&mask, "Task", Some(1000));
        let inner_vec = Value::unnamed_composite(vec![schedule_item]);
        let outer_vec = Value::unnamed_composite(vec![inner_vec]);

        let result = decode_reservations_from_value(&outer_vec);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].task, "1000");
    }

    #[test]
    fn test_decode_reservations_multiple() {
        // Two reservations
        let mask1 = vec![0xFF; 10];
        let item1 = make_schedule_item_value(&mask1, "Task", Some(1000));
        let inner1 = Value::unnamed_composite(vec![item1]);

        let mask2 = vec![0xAA; 10];
        let item2 = make_schedule_item_value(&mask2, "Pool", None);
        let inner2 = Value::unnamed_composite(vec![item2]);

        let outer_vec = Value::unnamed_composite(vec![inner1, inner2]);

        let result = decode_reservations_from_value(&outer_vec);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].task, "1000");
        assert_eq!(result[1].task, "Pool");
    }

    #[test]
    fn test_decode_reservations_invalid_assignment_variant() {
        // Unknown variant defaults to Idle
        let mask = vec![0xFF; 10];
        let mask_values: Vec<Value<()>> = mask.iter().map(|&b| Value::u128(b as u128)).collect();
        let schedule_item = Value::named_composite([
            ("mask", Value::unnamed_composite(mask_values)),
            (
                "assignment",
                Value::named_variant("Unknown", Vec::<(&str, Value<()>)>::new()),
            ),
        ]);
        let inner_vec = Value::unnamed_composite(vec![schedule_item]);
        let outer_vec = Value::unnamed_composite(vec![inner_vec]);

        let result = decode_reservations_from_value(&outer_vec);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].task, ""); // Defaults to Idle (empty string)
    }

    #[test]
    fn test_decode_reservations_truncated_mask() {
        // Empty mask - still works, just empty
        let schedule_item = Value::named_composite([
            ("mask", Value::unnamed_composite(Vec::<Value<()>>::new())),
            (
                "assignment",
                Value::named_variant("Idle", Vec::<(&str, Value<()>)>::new()),
            ),
        ]);
        let inner_vec = Value::unnamed_composite(vec![schedule_item]);
        let outer_vec = Value::unnamed_composite(vec![inner_vec]);

        let result = decode_reservations_from_value(&outer_vec);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].mask, "0x"); // Empty mask
    }

    #[test]
    fn test_decode_reservations_truncated_task_id() {
        // Task variant with no value - defaults to task ID 0
        let mask = vec![0xFF; 10];
        let mask_values: Vec<Value<()>> = mask.iter().map(|&b| Value::u128(b as u128)).collect();
        let schedule_item = Value::named_composite([
            ("mask", Value::unnamed_composite(mask_values)),
            (
                "assignment",
                Value::named_variant("Task", Vec::<(&str, Value<()>)>::new()),
            ),
        ]);
        let inner_vec = Value::unnamed_composite(vec![schedule_item]);
        let outer_vec = Value::unnamed_composite(vec![inner_vec]);

        let result = decode_reservations_from_value(&outer_vec);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].task, "0"); // Defaults to 0
    }

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
