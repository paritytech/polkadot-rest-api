// ================================================================================================
// Timestamp Data Fetching
// ================================================================================================

use subxt::{OnlineClientAtBlock, SubstrateConfig};

/// Fetch timestamp for a given block
pub async fn fetch_timestamp(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Option<String> {
    let timestamp_addr = subxt::dynamic::storage::<(), u64>("Timestamp", "Now");
    let timestamp = client_at_block
        .storage()
        .fetch(timestamp_addr, ())
        .await
        .ok()?;
    let timestamp_value = timestamp.decode().ok()?;
    Some(timestamp_value.to_string())
}
