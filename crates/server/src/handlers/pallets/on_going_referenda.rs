//! Handler for the `/pallets/on-going-referenda` endpoint.
//!
//! This endpoint returns all currently active (ongoing) referenda from the
//! Referenda pallet. Only relay chains (Polkadot, Kusama) support this endpoint
//! as parachains don't have governance.

use crate::handlers::pallets::common::{AtResponse, PalletError, format_account_id};
use crate::state::AppState;
use crate::utils;
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use futures::future::join_all;
use parity_scale_codec::Decode;
use serde::{Deserialize, Serialize};
use subxt::SubstrateConfig;

// ============================================================================
// Query Parameters
// ============================================================================

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnGoingReferendaQueryParams {
    /// Block height (number) or hash (0x-prefixed hex string)
    pub at: Option<String>,
    /// Use relay chain block (for Asset Hub)
    #[serde(default)]
    pub use_rc_block: bool,
}

// ============================================================================
// Response Types
// ============================================================================

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnGoingReferendaResponse {
    pub at: AtResponse,
    pub referenda: Vec<ReferendumInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}

/// Referendum info matching Sidecar's response format exactly
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferendumInfo {
    pub id: String,
    pub decision_deposit: Option<Deposit>,
    pub enactment: EnactmentInfo,
    pub submitted: String,
    pub deciding: Option<DecidingStatus>,
}

/// Enactment info matching Sidecar's format: {"after": "14400"} or {"at": "12345"}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnactmentInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Deposit {
    pub who: String,
    pub amount: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecidingStatus {
    pub since: String,
    pub confirming: Option<String>,
}

// ============================================================================
// Main Handler
// ============================================================================

/// Handler for GET /pallets/on-going-referenda
///
/// Returns all currently active referenda from the Referenda pallet.
pub async fn pallets_on_going_referenda(
    State(state): State<AppState>,
    Query(params): Query<OnGoingReferendaQueryParams>,
) -> Result<Response, PalletError> {
    // useRcBlock is not supported for this endpoint (relay chain only)
    if params.use_rc_block {
        return Err(PalletError::UseRcBlockNotSupported);
    }

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

    // Fetch all referenda from storage
    let referenda =
        fetch_ongoing_referenda(&client_at_block, state.chain_info.ss58_prefix, &at.height).await?;

    Ok((
        StatusCode::OK,
        Json(OnGoingReferendaResponse {
            at,
            referenda,
            rc_block_hash: None,
            rc_block_number: None,
            ah_timestamp: None,
        }),
    )
        .into_response())
}

// ============================================================================
// Storage Fetching
// ============================================================================

/// Fetch all ongoing referenda from the Referenda pallet storage
async fn fetch_ongoing_referenda(
    client_at_block: &subxt::client::OnlineClientAtBlock<SubstrateConfig>,
    ss58_prefix: u16,
    block_height: &str,
) -> Result<Vec<ReferendumInfo>, PalletError> {
    let mut referenda = Vec::new();

    // First, get the ReferendumCount to know how many referenda have been created
    let count_addr =
        subxt::dynamic::storage::<(), scale_value::Value>("Referenda", "ReferendumCount");
    let referendum_count: u32 = match client_at_block.storage().fetch(count_addr, ()).await {
        Ok(storage_val) => {
            // Decode the storage value to get the count
            let bytes = storage_val.into_bytes();
            match u32::decode(&mut &bytes[..]) {
                Ok(count) => {
                    tracing::info!("Successfully decoded ReferendumCount: {}", count);
                    count
                }
                Err(e) => {
                    tracing::warn!("Failed to decode ReferendumCount from bytes: {:?}", e);
                    return Err(PalletError::StorageDecodeFailed {
                        pallet: "Referenda",
                        entry: "ReferendumCount",
                    });
                }
            }
        }
        Err(e) => {
            // Check if this is because the pallet doesn't exist
            let error_str = format!("{:?}", e);
            if error_str.contains("Pallet")
                || error_str.contains("not found")
                || error_str.contains("Storage")
            {
                tracing::warn!(
                    "Referenda pallet not available at block {}: {:?}",
                    block_height,
                    e
                );
                return Err(PalletError::PalletNotAvailableAtBlock {
                    module: "api.query.referenda".to_string(),
                    block_height: block_height.to_string(),
                });
            }
            tracing::warn!("Failed to fetch ReferendumCount: {:?}", e);
            return Err(PalletError::StorageFetchFailed {
                pallet: "Referenda",
                entry: "ReferendumCount",
            });
        }
    };

    tracing::info!("ReferendumCount: {}", referendum_count);

    // Iterate in batches from highest ID to lowest (ongoing referenda are usually recent)
    // Use concurrent requests for better performance
    let batch_size = 50;
    let mut id = referendum_count.saturating_sub(1) as i64;

    while id >= 0 {
        let batch_start = (id - batch_size as i64 + 1).max(0) as u32;
        let batch_end = id as u32;

        // Create futures for batch fetching - decode immediately to avoid lifetime issues
        let futures: Vec<_> = (batch_start..=batch_end)
            .map(|ref_id| {
                let storage_addr = subxt::dynamic::storage::<_, scale_value::Value>(
                    "Referenda",
                    "ReferendumInfoFor",
                );
                let client = client_at_block.clone();
                async move {
                    let result = client.storage().fetch(storage_addr, (ref_id,)).await;
                    let decoded: Option<scale_value::Value<()>> = match result {
                        Ok(val) => val.decode_as().ok(),
                        Err(_) => None,
                    };
                    (ref_id, decoded)
                }
            })
            .collect();

        // Execute batch concurrently
        let results = join_all(futures).await;

        for (ref_id, decoded) in results {
            let decoded = match decoded {
                Some(d) => d,
                None => continue,
            };

            // Check if this is an Ongoing referendum with track 0 (Root) or track 1 (WhitelistedCaller)
            // This matches Sidecar's behavior which only returns these two tracks
            if let Some((track, ongoing)) =
                extract_ongoing_referendum_with_track(&decoded, ref_id, ss58_prefix)
            {
                // Filter to only include track 0 (Root) and track 1 (WhitelistedCaller)
                if track == "0" || track == "1" {
                    referenda.push(ongoing);
                }
            }
        }

        id -= batch_size as i64;
    }

    tracing::info!(
        "Found {} ongoing referenda out of {} total",
        referenda.len(),
        referendum_count
    );

    // Sort by ID in descending order to match Sidecar's ordering (highest ID first)
    referenda.sort_by(|a, b| {
        let a_id: u32 = a.id.replace(',', "").parse().unwrap_or(0);
        let b_id: u32 = b.id.replace(',', "").parse().unwrap_or(0);
        b_id.cmp(&a_id) // Descending order
    });

    Ok(referenda)
}

/// Extract ongoing referendum info from decoded storage value
/// Returns (track, ReferendumInfo) tuple for filtering
/// Returns data in Sidecar-compatible format
fn extract_ongoing_referendum_with_track(
    value: &scale_value::Value<()>,
    id: u32,
    ss58_prefix: u16,
) -> Option<(String, ReferendumInfo)> {
    // The value is an enum: Ongoing, Approved, Rejected, Cancelled, TimedOut, Killed
    // We only care about Ongoing variant
    //
    // The scale_value serializes as: {"name":"Ongoing","values":[{...struct fields...}]}
    // where values[0] contains the struct with named fields

    let value_json = scale_value_to_json(value);

    // Check if this is an Ongoing variant by looking at the "name" field
    let obj = value_json.as_object()?;
    let variant_name = obj.get("name")?.as_str()?;

    if variant_name != "Ongoing" {
        return None;
    }

    // Get the values array which contains one element: the Ongoing struct
    let values = obj.get("values")?.as_array()?;

    // The values array should have exactly one element containing the struct
    if values.is_empty() {
        tracing::debug!("Ongoing referendum {} has empty values array", id);
        return None;
    }

    // Get the struct object from values[0]
    let ongoing_obj = values[0].as_object()?;

    // Extract track for filtering (track 0 = Root, track 1 = WhitelistedCaller)
    let track = ongoing_obj
        .get("track")
        .map(extract_value_as_string)
        .unwrap_or_default();

    // Extract enactment in Sidecar format: {"after": "14400"} or {"at": "12345"}
    let enactment = extract_enactment_sidecar_format(ongoing_obj.get("enactment")?);

    let submitted = ongoing_obj
        .get("submitted")
        .map(extract_value_as_string)
        .unwrap_or_default();

    let decision_deposit = ongoing_obj
        .get("decision_deposit")
        .and_then(|v| extract_deposit_from_value(v, ss58_prefix));

    let deciding = ongoing_obj
        .get("deciding")
        .and_then(extract_deciding_from_value);

    // Format ID with comma like Sidecar does (e.g., "1,308" instead of "1308")
    let formatted_id = format_id_with_comma(id);

    Some((
        track,
        ReferendumInfo {
            id: formatted_id,
            decision_deposit,
            enactment,
            submitted,
            deciding,
        },
    ))
}

/// Format ID with comma separator like Sidecar (e.g., 1308 -> "1,308")
fn format_id_with_comma(id: u32) -> String {
    let s = id.to_string();
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(*c);
    }
    result
}

/// Extract enactment in Sidecar format: {"after": "14400"} or {"at": "12345"}
fn extract_enactment_sidecar_format(val: &serde_json::Value) -> EnactmentInfo {
    // The enactment is an enum: After(BlockNumber) or At(BlockNumber)
    // scale_value serializes as: {"name":"After","values":[14400]}

    if let Some(obj) = val.as_object() {
        let variant_name = obj.get("name").and_then(|n| n.as_str()).unwrap_or("");
        let values = obj.get("values").and_then(|v| v.as_array());

        if let Some(vals) = values
            && let Some(first) = vals.first()
        {
            let value_str = extract_value_as_string(first);

            match variant_name {
                "After" => {
                    return EnactmentInfo {
                        after: Some(value_str),
                        at: None,
                    };
                }
                "At" => {
                    return EnactmentInfo {
                        after: None,
                        at: Some(value_str),
                    };
                }
                _ => {}
            }
        }
    }

    // Default fallback
    EnactmentInfo {
        after: None,
        at: None,
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Convert scale_value::Value to serde_json::Value
fn scale_value_to_json(value: &scale_value::Value<()>) -> serde_json::Value {
    // Use serde to convert
    serde_json::to_value(value).unwrap_or(serde_json::Value::Null)
}

/// Check if a JSON value represents a None variant
fn is_none_variant(val: &serde_json::Value) -> bool {
    if let Some(obj) = val.as_object()
        && let Some(name) = obj.get("name")
    {
        return name.as_str() == Some("None");
    }
    false
}

/// Extract a value as a string (handles numbers, strings, and nested values)
fn extract_value_as_string(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Bool(b) => b.to_string(),
        _ => val.to_string(),
    }
}

/// Extract deposit from a value that might be Some or None variant
fn extract_deposit_from_value(val: &serde_json::Value, ss58_prefix: u16) -> Option<Deposit> {
    // Check if it's a None variant
    if val.is_null() || is_none_variant(val) {
        return None;
    }

    // Check if it's a Some variant with values
    if let Some(obj) = val.as_object()
        && obj.get("name").and_then(|n| n.as_str()) == Some("Some")
        && let Some(values) = obj.get("values").and_then(|v| v.as_array())
        && let Some(deposit_val) = values.first()
    {
        return extract_deposit_direct(deposit_val, ss58_prefix);
    }

    // Try direct extraction
    extract_deposit_direct(val, ss58_prefix)
}

/// Extract deposit directly from a deposit object
fn extract_deposit_direct(val: &serde_json::Value, ss58_prefix: u16) -> Option<Deposit> {
    let obj = val.as_object()?;

    // The deposit has "who" and "amount" fields
    let who_val = obj.get("who")?;
    let amount_val = obj.get("amount")?;

    let who = extract_account_from_value(who_val, ss58_prefix)?;
    let amount = extract_value_as_string(amount_val);

    Some(Deposit { who, amount })
}

/// Extract account ID from a value (handles nested arrays)
fn extract_account_from_value(val: &serde_json::Value, ss58_prefix: u16) -> Option<String> {
    // The account might be nested in an array like [[bytes...]]
    if let Some(arr) = val.as_array() {
        if arr.len() == 1
            && let Some(inner_arr) = arr[0].as_array()
        {
            // It's [[byte, byte, ...]]
            let bytes: Vec<u8> = inner_arr
                .iter()
                .filter_map(|v| v.as_u64().map(|n| n as u8))
                .collect();
            if bytes.len() == 32 {
                let bytes_arr: [u8; 32] = bytes.try_into().ok()?;
                return Some(format_account_id(&bytes_arr, ss58_prefix));
            }
        }
        // It's [byte, byte, ...]
        let bytes: Vec<u8> = arr
            .iter()
            .filter_map(|v| v.as_u64().map(|n| n as u8))
            .collect();
        if bytes.len() == 32 {
            let bytes_arr: [u8; 32] = bytes.try_into().ok()?;
            return Some(format_account_id(&bytes_arr, ss58_prefix));
        }
    }
    None
}

/// Extract deciding status from a value
fn extract_deciding_from_value(val: &serde_json::Value) -> Option<DecidingStatus> {
    // Check if it's a None variant
    if val.is_null() || is_none_variant(val) {
        return None;
    }

    // Check if it's a Some variant with values
    if let Some(obj) = val.as_object()
        && obj.get("name").and_then(|n| n.as_str()) == Some("Some")
        && let Some(values) = obj.get("values").and_then(|v| v.as_array())
        && let Some(deciding_val) = values.first()
    {
        return extract_deciding_direct(deciding_val);
    }

    extract_deciding_direct(val)
}

/// Extract deciding status directly
fn extract_deciding_direct(val: &serde_json::Value) -> Option<DecidingStatus> {
    let obj = val.as_object()?;

    let since_val = obj.get("since")?;
    let since = extract_value_as_string(since_val);

    let confirming = if let Some(confirming_val) = obj.get("confirming") {
        if is_none_variant(confirming_val) || confirming_val.is_null() {
            None
        } else if let Some(obj) = confirming_val.as_object() {
            if obj.get("name").and_then(|n| n.as_str()) == Some("Some") {
                obj.get("values")
                    .and_then(|v| v.as_array())
                    .and_then(|arr| arr.first())
                    .map(extract_value_as_string)
            } else {
                None
            }
        } else {
            Some(extract_value_as_string(confirming_val))
        }
    } else {
        None
    };

    Some(DecidingStatus { since, confirming })
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ========================================================================
    // format_id_with_comma tests
    // ========================================================================

    #[test]
    fn test_format_id_with_comma_single_digit() {
        assert_eq!(format_id_with_comma(1), "1");
        assert_eq!(format_id_with_comma(9), "9");
    }

    #[test]
    fn test_format_id_with_comma_double_digit() {
        assert_eq!(format_id_with_comma(10), "10");
        assert_eq!(format_id_with_comma(99), "99");
    }

    #[test]
    fn test_format_id_with_comma_triple_digit() {
        assert_eq!(format_id_with_comma(100), "100");
        assert_eq!(format_id_with_comma(999), "999");
    }

    #[test]
    fn test_format_id_with_comma_four_digits() {
        assert_eq!(format_id_with_comma(1000), "1,000");
        assert_eq!(format_id_with_comma(1308), "1,308");
        assert_eq!(format_id_with_comma(1339), "1,339");
        assert_eq!(format_id_with_comma(1349), "1,349");
        assert_eq!(format_id_with_comma(9999), "9,999");
    }

    #[test]
    fn test_format_id_with_comma_large_numbers() {
        assert_eq!(format_id_with_comma(10000), "10,000");
        assert_eq!(format_id_with_comma(100000), "100,000");
        assert_eq!(format_id_with_comma(1000000), "1,000,000");
        assert_eq!(format_id_with_comma(1234567), "1,234,567");
    }

    #[test]
    fn test_format_id_with_comma_zero() {
        assert_eq!(format_id_with_comma(0), "0");
    }

    // ========================================================================
    // is_none_variant tests
    // ========================================================================

    #[test]
    fn test_is_none_variant_true() {
        let none_val = json!({"name": "None", "values": []});
        assert!(is_none_variant(&none_val));
    }

    #[test]
    fn test_is_none_variant_false_some() {
        let some_val = json!({"name": "Some", "values": [123]});
        assert!(!is_none_variant(&some_val));
    }

    #[test]
    fn test_is_none_variant_false_other() {
        let other_val = json!({"name": "Ongoing", "values": []});
        assert!(!is_none_variant(&other_val));
    }

    #[test]
    fn test_is_none_variant_false_null() {
        let null_val = json!(null);
        assert!(!is_none_variant(&null_val));
    }

    #[test]
    fn test_is_none_variant_false_no_name() {
        let no_name = json!({"values": []});
        assert!(!is_none_variant(&no_name));
    }

    // ========================================================================
    // extract_value_as_string tests
    // ========================================================================

    #[test]
    fn test_extract_value_as_string_number() {
        let num = json!(12345);
        assert_eq!(extract_value_as_string(&num), "12345");
    }

    #[test]
    fn test_extract_value_as_string_string() {
        let s = json!("hello");
        assert_eq!(extract_value_as_string(&s), "hello");
    }

    #[test]
    fn test_extract_value_as_string_bool() {
        let b = json!(true);
        assert_eq!(extract_value_as_string(&b), "true");
    }

    #[test]
    fn test_extract_value_as_string_large_number() {
        let num = json!(1000000000000000_u64);
        assert_eq!(extract_value_as_string(&num), "1000000000000000");
    }

    // ========================================================================
    // extract_enactment_sidecar_format tests
    // ========================================================================

    #[test]
    fn test_extract_enactment_after() {
        let val = json!({"name": "After", "values": [14400]});
        let result = extract_enactment_sidecar_format(&val);
        assert_eq!(result.after, Some("14400".to_string()));
        assert_eq!(result.at, None);
    }

    #[test]
    fn test_extract_enactment_at() {
        let val = json!({"name": "At", "values": [12345]});
        let result = extract_enactment_sidecar_format(&val);
        assert_eq!(result.after, None);
        assert_eq!(result.at, Some("12345".to_string()));
    }

    #[test]
    fn test_extract_enactment_unknown_variant() {
        let val = json!({"name": "Unknown", "values": [100]});
        let result = extract_enactment_sidecar_format(&val);
        assert_eq!(result.after, None);
        assert_eq!(result.at, None);
    }

    #[test]
    fn test_extract_enactment_empty_values() {
        let val = json!({"name": "After", "values": []});
        let result = extract_enactment_sidecar_format(&val);
        assert_eq!(result.after, None);
        assert_eq!(result.at, None);
    }

    #[test]
    fn test_extract_enactment_null() {
        let val = json!(null);
        let result = extract_enactment_sidecar_format(&val);
        assert_eq!(result.after, None);
        assert_eq!(result.at, None);
    }

    // ========================================================================
    // extract_deciding_from_value tests
    // ========================================================================

    #[test]
    fn test_extract_deciding_none_variant() {
        let val = json!({"name": "None", "values": []});
        assert!(extract_deciding_from_value(&val).is_none());
    }

    #[test]
    fn test_extract_deciding_null() {
        let val = json!(null);
        assert!(extract_deciding_from_value(&val).is_none());
    }

    #[test]
    fn test_extract_deciding_some_variant() {
        let val = json!({
            "name": "Some",
            "values": [{
                "since": 23687165,
                "confirming": {"name": "None", "values": []}
            }]
        });
        let result = extract_deciding_from_value(&val);
        assert!(result.is_some());
        let deciding = result.unwrap();
        assert_eq!(deciding.since, "23687165");
        assert!(deciding.confirming.is_none());
    }

    #[test]
    fn test_extract_deciding_direct() {
        let val = json!({
            "since": 23687165,
            "confirming": {"name": "None", "values": []}
        });
        let result = extract_deciding_from_value(&val);
        assert!(result.is_some());
        let deciding = result.unwrap();
        assert_eq!(deciding.since, "23687165");
        assert!(deciding.confirming.is_none());
    }

    #[test]
    fn test_extract_deciding_with_confirming() {
        let val = json!({
            "since": 23687165,
            "confirming": {"name": "Some", "values": [24000000]}
        });
        let result = extract_deciding_from_value(&val);
        assert!(result.is_some());
        let deciding = result.unwrap();
        assert_eq!(deciding.since, "23687165");
        assert_eq!(deciding.confirming, Some("24000000".to_string()));
    }

    // ========================================================================
    // extract_deposit_from_value tests
    // ========================================================================

    #[test]
    fn test_extract_deposit_none_variant() {
        let val = json!({"name": "None", "values": []});
        assert!(extract_deposit_from_value(&val, 0).is_none());
    }

    #[test]
    fn test_extract_deposit_null() {
        let val = json!(null);
        assert!(extract_deposit_from_value(&val, 0).is_none());
    }

    #[test]
    fn test_extract_deposit_some_variant() {
        // Use a known account ID (32 bytes)
        let account_bytes: Vec<u8> = vec![
            0x8e, 0xaf, 0x04, 0x15, 0x16, 0x87, 0x73, 0x63, 0x26, 0xc9, 0xfe, 0xa1, 0x7e, 0x25,
            0xfc, 0x52, 0x87, 0x61, 0x36, 0x93, 0xc9, 0x12, 0x90, 0x9c, 0xb2, 0x26, 0xaa, 0x47,
            0x94, 0xf2, 0x6a, 0x48,
        ];
        let val = json!({
            "name": "Some",
            "values": [{
                "who": [account_bytes],
                "amount": "1000000000000000"
            }]
        });
        let result = extract_deposit_from_value(&val, 0);
        assert!(result.is_some());
        let deposit = result.unwrap();
        assert_eq!(deposit.amount, "1000000000000000");
        // Account should be SS58 encoded
        assert!(!deposit.who.is_empty());
    }

    // ========================================================================
    // extract_account_from_value tests
    // ========================================================================

    #[test]
    fn test_extract_account_flat_array() {
        // 32 bytes as flat array
        let account_bytes: Vec<serde_json::Value> = (0..32).map(|i| json!(i as u8)).collect();
        let val = json!(account_bytes);
        let result = extract_account_from_value(&val, 0);
        assert!(result.is_some());
    }

    #[test]
    fn test_extract_account_nested_array() {
        // 32 bytes as nested array [[...]]
        let account_bytes: Vec<serde_json::Value> = (0..32).map(|i| json!(i as u8)).collect();
        let val = json!([account_bytes]);
        let result = extract_account_from_value(&val, 0);
        assert!(result.is_some());
    }

    #[test]
    fn test_extract_account_wrong_length() {
        // Only 16 bytes - should fail
        let account_bytes: Vec<serde_json::Value> = (0..16).map(|i| json!(i as u8)).collect();
        let val = json!(account_bytes);
        let result = extract_account_from_value(&val, 0);
        assert!(result.is_none());
    }

    #[test]
    fn test_extract_account_not_array() {
        let val = json!("not an array");
        let result = extract_account_from_value(&val, 0);
        assert!(result.is_none());
    }

    // ========================================================================
    // Response serialization tests
    // ========================================================================

    #[test]
    fn test_referendum_info_serialization() {
        let referendum = ReferendumInfo {
            id: "1,308".to_string(),
            decision_deposit: Some(Deposit {
                who: "13sDzot2hwoEAzXJiNe3cBiMEq19XRqrS3DMAxt9jiSNKMkA".to_string(),
                amount: "1000000000000000".to_string(),
            }),
            enactment: EnactmentInfo {
                after: Some("14400".to_string()),
                at: None,
            },
            submitted: "23496576".to_string(),
            deciding: Some(DecidingStatus {
                since: "23687165".to_string(),
                confirming: None,
            }),
        };

        let json = serde_json::to_value(&referendum).unwrap();
        assert_eq!(json["id"], "1,308");
        assert_eq!(json["decisionDeposit"]["amount"], "1000000000000000");
        assert_eq!(json["enactment"]["after"], "14400");
        assert!(json["enactment"].get("at").is_none());
        assert_eq!(json["submitted"], "23496576");
        assert_eq!(json["deciding"]["since"], "23687165");
    }

    #[test]
    fn test_referendum_info_null_fields() {
        let referendum = ReferendumInfo {
            id: "1,349".to_string(),
            decision_deposit: None,
            enactment: EnactmentInfo {
                after: Some("100".to_string()),
                at: None,
            },
            submitted: "23810220".to_string(),
            deciding: None,
        };

        let json = serde_json::to_value(&referendum).unwrap();
        assert_eq!(json["id"], "1,349");
        assert!(json["decisionDeposit"].is_null());
        assert!(json["deciding"].is_null());
    }

    #[test]
    fn test_enactment_at_variant() {
        let enactment = EnactmentInfo {
            after: None,
            at: Some("25000000".to_string()),
        };

        let json = serde_json::to_value(&enactment).unwrap();
        assert!(json.get("after").is_none());
        assert_eq!(json["at"], "25000000");
    }

    // ========================================================================
    // Query params tests
    // ========================================================================

    #[test]
    fn test_query_params_default() {
        let params: OnGoingReferendaQueryParams = serde_json::from_str("{}").unwrap();
        assert!(params.at.is_none());
        assert!(!params.use_rc_block);
    }

    #[test]
    fn test_query_params_with_at() {
        let params: OnGoingReferendaQueryParams =
            serde_json::from_str(r#"{"at": "24000000"}"#).unwrap();
        assert_eq!(params.at, Some("24000000".to_string()));
        assert!(!params.use_rc_block);
    }

    #[test]
    fn test_query_params_with_use_rc_block() {
        let params: OnGoingReferendaQueryParams =
            serde_json::from_str(r#"{"useRcBlock": true}"#).unwrap();
        assert!(params.at.is_none());
        assert!(params.use_rc_block);
    }
}
