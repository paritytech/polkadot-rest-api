use serde::{Deserialize, Serialize};

/// Common query parameter for specifying a block to query at
#[derive(Debug, Deserialize)]
pub struct AtBlockParam {
    pub at: Option<String>,
}

/// Common response structure for block information
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockInfo {
    pub hash: String,
    pub height: String,
}
