// ================================================================================================
// Timestamp Data Fetching
// ================================================================================================

use crate::state::AppState;
use parity_scale_codec::Decode;

/// Fetch timestamp for a given block
pub async fn fetch_timestamp(state: &AppState, block_number: u64) -> Result<String, Box<dyn std::error::Error>> {
    let client_at_block = state.client.at_block(block_number).await?;

    if let Ok(timestamp_entry) = client_at_block.storage().entry(subxt::storage::dynamic::<Vec<scale_value::Value>, scale_value::Value>("Timestamp", "Now")) {
        if let Ok(Some(timestamp)) = timestamp_entry.try_fetch(Vec::<scale_value::Value>::new()).await {
            // Timestamp is a u64 (milliseconds) - decode from storage value
            let timestamp_bytes = timestamp.into_bytes();
            let mut cursor = &timestamp_bytes[..];
            if let Ok(timestamp_value) = u64::decode(&mut cursor) {
                return Ok(timestamp_value.to_string());
            }
        }
    }

    Err("Failed to fetch timestamp".into())
}