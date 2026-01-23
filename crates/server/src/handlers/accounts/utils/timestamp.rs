// ================================================================================================
// Timestamp Data Fetching
// ================================================================================================

use parity_scale_codec::Decode;
use scale_value::Value;
use subxt::{OnlineClientAtBlock, SubstrateConfig};

/// Fetch timestamp for a given block
pub async fn fetch_timestamp(client_at_block: &OnlineClientAtBlock<SubstrateConfig>,) -> Result<String, Box<dyn std::error::Error>> {
    let storage_query = subxt::storage::dynamic::<Vec<Value>, Value>("Timestamp", "Now");

    if let Ok(storage_entry) = client_at_block.storage().entry(storage_query) {
        if let Ok(Some(timestamp)) = storage_entry.try_fetch(Vec::<Value>::new()).await {
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