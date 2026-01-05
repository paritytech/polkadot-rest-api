//! Shared types for block-related handlers.
//!
//! This module contains all the types used by `/blocks/*` endpoints including
//! request parameters, response structures, and internal types.

use crate::utils::{self, EraInfo, RcBlockError};
use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use subxt_historic::error::{OnlineClientAtBlockError, StorageEntryIsNotAPlainValue, StorageError};
use thiserror::Error;

// ================================================================================================
// Constants
// ================================================================================================

/// Length of consensus engine ID in digest items (e.g., "BABE", "aura", "pow_")
pub const CONSENSUS_ENGINE_ID_LEN: usize = 4;

// ================================================================================================
// Query Parameters
// ================================================================================================

/// Query parameters for /blocks/{blockId} endpoint
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockQueryParams {
    /// When true, include documentation for events
    #[serde(default)]
    pub event_docs: bool,
    /// When true, include documentation for extrinsics
    #[serde(default)]
    pub extrinsic_docs: bool,
    /// When true, skip fee calculation for extrinsics (info will be empty object)
    #[serde(default)]
    pub no_fees: bool,
    /// When true, include finalized status in response. When false, omit finalized field.
    #[serde(default = "default_true")]
    pub finalized_key: bool,
    /// When true, treat block identifier as Relay Chain block and return Asset Hub blocks included in it
    #[serde(default)]
    pub use_rc_block: bool,
    /// When true, decode and include XCM messages from the block's extrinsics
    #[serde(default)]
    pub decoded_xcm_msgs: bool,
    /// Filter decoded XCM messages by parachain ID (only used when decodedXcmMsgs=true)
    #[serde(default)]
    pub para_id: Option<u32>,
}

fn default_true() -> bool {
    true
}

impl Default for BlockQueryParams {
    fn default() -> Self {
        Self {
            event_docs: false,
            extrinsic_docs: false,
            no_fees: false,
            finalized_key: true,
            use_rc_block: false,
            decoded_xcm_msgs: false,
            para_id: None,
        }
    }
}

// ================================================================================================
// Error Types
// ================================================================================================

#[derive(Debug, Error)]
pub enum GetBlockError {
    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] utils::BlockIdParseError),

    #[error("Block resolution failed")]
    BlockResolveFailed(#[from] utils::BlockResolveError),

    #[error("Failed to get block header")]
    HeaderFetchFailed(#[source] subxt_rpcs::Error),

    #[error("Header field missing: {0}")]
    HeaderFieldMissing(String),

    #[error("Failed to get client at block: {0}")]
    ClientAtBlockFailed(#[from] OnlineClientAtBlockError),

    #[error("Failed to fetch chain storage")]
    StorageFetchFailed(#[from] StorageError),

    #[error("Storage entry is not a plain value")]
    StorageNotPlainValue(#[from] StorageEntryIsNotAPlainValue),

    #[error("Failed to decode storage value")]
    StorageDecodeFailed(#[from] parity_scale_codec::Error),

    #[error("Failed to fetch extrinsics")]
    ExtrinsicsFetchFailed(String),

    #[error("Missing signature bytes for signed extrinsic")]
    MissingSignatureBytes,

    #[error("Missing address bytes for signed extrinsic")]
    MissingAddressBytes,

    #[error("Failed to decode extrinsic field: {0}")]
    ExtrinsicDecodeFailed(String),

    #[error("Failed to get finalized head")]
    FinalizedHeadFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to get canonical block hash")]
    CanonicalHashFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to find Asset Hub blocks in Relay Chain block")]
    RcBlockError(#[from] RcBlockError),

    #[error("useRcBlock parameter is only supported for Asset Hub endpoints")]
    UseRcBlockNotSupported,

    #[error(
        "useRcBlock parameter requires relay chain API to be available. Please configure SAS_SUBSTRATE_MULTI_CHAIN_URL"
    )]
    RelayChainNotConfigured,

    #[error("Failed to compute block hash: {0}")]
    HashComputationFailed(#[from] crate::utils::HashError),

    #[error("Failed to get runtime version")]
    RuntimeVersionFailed(#[source] subxt_rpcs::Error),

    #[error("Service temporarily unavailable: {0}")]
    ServiceUnavailable(String),
}

impl IntoResponse for GetBlockError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            GetBlockError::InvalidBlockParam(_) | GetBlockError::BlockResolveFailed(_) | GetBlockError::RelayChainNotConfigured(_) => {
                (StatusCode::BAD_REQUEST, self.to_string())
            }
            GetBlockError::ServiceUnavailable(_) => {
                (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
            }
            // Check if ClientAtBlockFailed is due to a disconnection
            GetBlockError::ClientAtBlockFailed(err)
                if utils::is_online_client_at_block_disconnected(err) =>
            {
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    format!("Service temporarily unavailable: {}", self),
                )
            }
            // Handle RPC errors with appropriate status codes
            GetBlockError::HeaderFetchFailed(err)
            | GetBlockError::FinalizedHeadFailed(err)
            | GetBlockError::CanonicalHashFailed(err)
            | GetBlockError::RuntimeVersionFailed(err) => utils::rpc_error_to_status(err),
            // All other errors are internal server errors
            GetBlockError::HeaderFieldMissing(_)
            | GetBlockError::ClientAtBlockFailed(_)
            | GetBlockError::StorageFetchFailed(_)
            | GetBlockError::StorageNotPlainValue(_)
            | GetBlockError::StorageDecodeFailed(_)
            | GetBlockError::ExtrinsicsFetchFailed(_)
            | GetBlockError::MissingSignatureBytes
            | GetBlockError::MissingAddressBytes
            | GetBlockError::ExtrinsicDecodeFailed(_)
            | GetBlockError::FinalizedHeadFailed(_)
            | GetBlockError::CanonicalHashFailed(_)
            | GetBlockError::RcBlockError(_)
            | GetBlockError::UseRcBlockNotSupported
            | GetBlockError::HashComputationFailed(_) => {
                (StatusCode::INTERNAL_SERVER_ERROR, self.to_string())
            }
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

// ================================================================================================
// Internal Enums
// ================================================================================================

/// SCALE encoding discriminants for the DigestItem enum from sp_runtime::generic
///
/// These discriminants match the SCALE encoding of substrate's DigestItem enum.
/// Reference: sp_runtime::generic::DigestItem
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DigestItemDiscriminant {
    /// ChangesTrieRoot has been removed but was 2
    /// ChangesTrieSignal has been removed but was 3
    Other = 0,
    Consensus = 4,
    Seal = 5,
    PreRuntime = 6,
    RuntimeEnvironmentUpdated = 8,
}

impl TryFrom<u8> for DigestItemDiscriminant {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Other),
            4 => Ok(Self::Consensus),
            5 => Ok(Self::Seal),
            6 => Ok(Self::PreRuntime),
            8 => Ok(Self::RuntimeEnvironmentUpdated),
            _ => Err(()),
        }
    }
}

impl DigestItemDiscriminant {
    /// Convert discriminant to string representation for JSON serialization
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Other => "Other",
            Self::Consensus => "Consensus",
            Self::Seal => "Seal",
            Self::PreRuntime => "PreRuntime",
            Self::RuntimeEnvironmentUpdated => "RuntimeEnvironmentUpdated",
        }
    }
}

/// MultiAddress type for decoding Substrate address variants
/// This represents the different ways an address can be encoded in Substrate
#[derive(scale_decode::DecodeAsType)]
#[allow(dead_code)]
pub enum MultiAddress {
    /// An AccountId32 (32 bytes)
    Id([u8; 32]),
    /// An account index
    Index(u32),
    /// Raw bytes
    Raw(Vec<u8>),
    /// A 32-byte address
    Address32([u8; 32]),
    /// A 20-byte address (Ethereum-style)
    Address20([u8; 20]),
}

/// Event phase - when during block execution the event was emitted
#[derive(Debug, Clone)]
pub enum EventPhase {
    /// During block initialization
    Initialization,
    /// During extrinsic application (contains extrinsic index)
    ApplyExtrinsic(u32),
    /// During block finalization
    Finalization,
}

// ================================================================================================
// Response Structs
// ================================================================================================

/// Represents a decoded digest log entry
#[derive(Debug, Serialize)]
pub struct DigestLog {
    #[serde(rename = "type")]
    pub log_type: String,
    pub index: String,
    pub value: Value,
}

/// Method information for extrinsic calls
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MethodInfo {
    pub pallet: String,
    pub method: String,
}

/// Event information in block response
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub method: MethodInfo,
    pub data: Vec<Value>,
    /// Documentation for this event (only present when eventDocs=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
}

/// Events that occurred during block initialization
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnInitialize {
    pub events: Vec<Event>,
}

/// Events that occurred during block finalization
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnFinalize {
    pub events: Vec<Event>,
}

/// Signer ID wrapper matching sidecar format
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignerId {
    pub id: String,
}

/// Signature information for signed extrinsics
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureInfo {
    pub signature: String,
    pub signer: SignerId,
}

/// Extrinsic information matching sidecar format
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtrinsicInfo {
    pub method: MethodInfo,
    /// Signature information - null for unsigned extrinsics (inherents)
    pub signature: Option<SignatureInfo>,
    /// Nonce - shown as null when extraction fails (matching sidecar behavior)
    pub nonce: Option<String>,
    /// Args as a JSON object where bytes are hex-encoded and large numbers are strings
    pub args: serde_json::Map<String, Value>,
    /// Tip - shown as null when extraction fails (matching sidecar behavior)
    pub tip: Option<String>,
    pub hash: String,
    /// Runtime dispatch info containing weight, class, and partialFee for signed extrinsics
    pub info: serde_json::Map<String, Value>,
    /// Transaction era/mortality information
    pub era: EraInfo,
    /// Events emitted by this extrinsic
    pub events: Vec<Event>,
    /// Whether the extrinsic executed successfully (determined from System.ExtrinsicSuccess event)
    pub success: bool,
    /// Whether the extrinsic pays a fee (None for unsigned, Some(bool) for signed)
    /// Extracted from DispatchInfo in System.ExtrinsicSuccess/ExtrinsicFailed events
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pays_fee: Option<bool>,
    /// Documentation for this extrinsic (only present when extrinsicDocs=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
    /// Raw extrinsic bytes as hex (used internally for fee queries, not serialized)
    #[serde(skip)]
    pub raw_hex: String,
}

/// Basic block information
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockResponse {
    pub number: String,
    pub hash: String,
    pub parent_hash: String,
    pub state_root: String,
    pub extrinsics_root: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author_id: Option<String>,
    pub logs: Vec<DigestLog>,
    pub on_initialize: OnInitialize,
    pub extrinsics: Vec<ExtrinsicInfo>,
    pub on_finalize: OnFinalize,
    /// Whether this block has been finalized (omitted when finalizedKey=false)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finalized: Option<bool>,
    /// Relay Chain block hash (only present when useRcBlock=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,
    /// Relay Chain block number (only present when useRcBlock=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,
    /// Asset Hub block timestamp (only present when useRcBlock=true)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
    /// Decoded XCM messages (omitted when decodedXcmMsgs=false)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decoded_xcm_msgs: Option<XcmMessages>,
}

// ================================================================================================
// XCM Message Types
// ================================================================================================

/// Container for decoded XCM messages from a block
#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct XcmMessages {
    pub horizontal_messages: Vec<HorizontalMessage>,
    pub downward_messages: Vec<DownwardMessage>,
    pub upward_messages: Vec<UpwardMessage>,
}

/// Upward message from a parachain to the relay chain
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpwardMessage {
    pub origin_para_id: String,
    pub data: Value,
}

/// Downward message from the relay chain to a parachain
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownwardMessage {
    pub sent_at: String,
    pub msg: String,
    pub data: Value,
}

/// Horizontal message between parachains
/// Format differs slightly between relay chain and parachain perspective
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HorizontalMessage {
    pub origin_para_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_para_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sent_at: Option<String>,
    pub data: Value,
}

// ================================================================================================
// Internal Structs
// ================================================================================================

/// A parsed event with its phase information
#[derive(Debug)]
pub struct ParsedEvent {
    /// When in the block this event occurred
    pub phase: EventPhase,
    /// Event pallet name
    pub pallet_name: String,
    /// Event variant name
    pub event_name: String,
    /// Event data as JSON
    pub event_data: Vec<Value>,
}

/// Weight information extracted from DispatchInfo in events
/// Can be either a single value (legacy) or ref_time + proof_size (modern)
#[derive(Debug, Default, Clone)]
pub struct ActualWeight {
    /// The ref_time component (or the single weight value for legacy format)
    pub ref_time: Option<String>,
    /// The proof_size component (None for legacy weight format)
    pub proof_size: Option<String>,
}

/// Outcome information for an extrinsic (success and paysFee)
/// Extracted from System.ExtrinsicSuccess or System.ExtrinsicFailed events
#[derive(Debug, Default, Clone)]
pub struct ExtrinsicOutcome {
    /// Whether the extrinsic succeeded (true if ExtrinsicSuccess event found)
    pub success: bool,
    /// Whether the extrinsic pays a fee (extracted from DispatchInfo)
    /// None means we couldn't determine it from events
    pub pays_fee: Option<bool>,
    /// Actual weight used during extrinsic execution (from DispatchInfo)
    /// This is needed for accurate fee calculation with calc_partial_fee
    pub actual_weight: Option<ActualWeight>,
    /// Dispatch class (Normal, Operational, or Mandatory)
    pub class: Option<String>,
}
