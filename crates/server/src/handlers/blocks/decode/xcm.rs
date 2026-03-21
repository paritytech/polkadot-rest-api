// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! XCM message decoding for block extrinsics.
//!
//! This module provides:
//! - `XcmDecoder` for extracting and decoding XCM messages from extrinsics
//! - `scale_value_to_json` for registry-aware conversion of SCALE values to JSON
//! - `ParachainMetadataCache` for fetching and caching parachain runtime metadata
//!   to decode non-XCM UMP messages using the sending parachain's type registry
//!
//! ## Decoding Strategy
//!
//! UMP (Upward Message Passing) channels carry `Vec<u8>` — parachains can send
//! any bytes, not just `VersionedXcm`. The decoder uses a tiered approach:
//!
//! 1. **XCM decode** — Try `VersionedXcm` (V3/V4/V5) via a local type registry
//! 2. **Parachain metadata decode** — If XCM fails and a parachain RPC URL is
//!    configured, fetch the parachain's runtime metadata and attempt to decode
//!    using candidate types found in the `PortableRegistry`
//! 3. **Fallback** — Return raw hex with a `decodingNote` explaining the failure

use heck::ToLowerCamelCase;
use scale_info::{PortableRegistry, TypeDef};
use scale_value::scale::decode_as_type;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::super::types::{
    DownwardMessage, ExtrinsicInfo, HorizontalMessage, UpwardMessage, XcmMessages,
};
use polkadot_rest_api_config::ChainType;

// ================================================================================================
// Registry-Aware SCALE Value Transformation
// ================================================================================================

/// Check if a type_id refers to a sequence type (Vec<T>) in the registry.
/// This is used to distinguish between sequences (which should stay as arrays)
/// and newtype wrappers (which should be unwrapped).
pub fn is_sequence_type(type_id: u32, registry: &PortableRegistry) -> bool {
    registry
        .resolve(type_id)
        .is_some_and(|ty| matches!(ty.type_def, TypeDef::Sequence(_)))
}

/// Check if an array of scale_value::Value looks like a byte array (all u8 values 0-255).
/// Requires at least 2 elements to avoid treating single compact integers as byte arrays.
pub fn is_byte_array_scale_value(values: &[scale_value::Value<u32>]) -> bool {
    values.len() >= 2
        && values.iter().all(|v| {
            matches!(
                &v.value,
                scale_value::ValueDef::Primitive(scale_value::Primitive::U128(n)) if *n <= 255
            )
        })
}

/// Convert a slice of scale_value::Value (representing bytes) to a hex string.
pub fn bytes_to_hex_scale_value(values: &[scale_value::Value<u32>]) -> String {
    let bytes: Vec<u8> = values
        .iter()
        .filter_map(|v| match &v.value {
            scale_value::ValueDef::Primitive(scale_value::Primitive::U128(n)) => Some(*n as u8),
            _ => None,
        })
        .collect();
    format!("0x{}", hex::encode(bytes))
}

/// Check if variant name is an X1-X8 junction.
/// These variants need special handling to preserve array output format.
/// Note: X1 is included here (unlike args.rs) because decoded XCM messages
/// represent X1 as an array to match sidecar's output format for XCM instructions.
fn is_junction_variant(name: &str) -> bool {
    matches!(name, "X1" | "X2" | "X3" | "X4" | "X5" | "X6" | "X7" | "X8")
}

/// Convert a scale_value::Value<u32> to serde_json::Value with registry awareness.
///
/// This correctly handles:
/// - Vec<T> (sequences) - always keeps as array even with single element
/// - Newtype wrappers - unwraps single unnamed field
/// - Byte arrays - converts to hex strings
/// - Named structs - converts to JSON objects with camelCase keys
/// - Enum variants - converts to { "variantName": value } format
///
/// The key insight: the decision to unwrap is based on the TYPE (checking TypeDef::Sequence
/// in the registry), not the array length.
pub fn scale_value_to_json(value: scale_value::Value<u32>, registry: &PortableRegistry) -> Value {
    let type_id = value.context;
    let is_sequence = is_sequence_type(type_id, registry);

    match value.value {
        scale_value::ValueDef::Composite(composite) => match composite {
            scale_value::Composite::Named(fields) => {
                let map: serde_json::Map<String, Value> = fields
                    .into_iter()
                    .map(|(name, val)| {
                        (
                            name.to_lower_camel_case(),
                            scale_value_to_json(val, registry),
                        )
                    })
                    .collect();
                Value::Object(map)
            }
            scale_value::Composite::Unnamed(fields) => {
                let fields_vec: Vec<_> = fields.into_iter().collect();
                // Check if this looks like a byte array
                if !fields_vec.is_empty() && is_byte_array_scale_value(&fields_vec) {
                    Value::String(bytes_to_hex_scale_value(&fields_vec))
                } else if fields_vec.len() == 1 && !is_sequence {
                    // Single unnamed field that's NOT a sequence - unwrap it (newtype wrapper)
                    match fields_vec.into_iter().next() {
                        Some(field) => scale_value_to_json(field, registry),
                        None => Value::Null,
                    }
                } else {
                    // Sequence type or multiple elements - keep as array
                    Value::Array(
                        fields_vec
                            .into_iter()
                            .map(|v| scale_value_to_json(v, registry))
                            .collect(),
                    )
                }
            }
        },
        scale_value::ValueDef::Variant(variant) => {
            // Handle Option::None as JSON null
            if variant.name == "None" {
                return Value::Null;
            }

            let name = variant.name.to_lower_camel_case();
            let is_junction = is_junction_variant(&variant.name);

            let inner = match variant.values {
                scale_value::Composite::Named(fields) if !fields.is_empty() => {
                    let map: serde_json::Map<String, Value> = fields
                        .into_iter()
                        .map(|(n, v)| (n.to_lower_camel_case(), scale_value_to_json(v, registry)))
                        .collect();
                    Value::Object(map)
                }
                scale_value::Composite::Unnamed(fields) if !fields.is_empty() => {
                    let fields_vec: Vec<_> = fields.into_iter().collect();
                    if !fields_vec.is_empty() && is_byte_array_scale_value(&fields_vec) {
                        Value::String(bytes_to_hex_scale_value(&fields_vec))
                    } else if fields_vec.len() == 1 && !is_junction {
                        // Single unnamed field - recurse into it
                        match fields_vec.into_iter().next() {
                            Some(field) => scale_value_to_json(field, registry),
                            None => Value::Null,
                        }
                    } else {
                        // For junctions (X1, X2, etc) or multi-element, output as array
                        Value::Array(
                            fields_vec
                                .into_iter()
                                .map(|v| scale_value_to_json(v, registry))
                                .collect(),
                        )
                    }
                }
                _ => Value::Null,
            };
            let mut map = serde_json::Map::new();
            map.insert(name, inner);
            Value::Object(map)
        }
        scale_value::ValueDef::Primitive(prim) => match prim {
            scale_value::Primitive::Bool(b) => Value::Bool(b),
            scale_value::Primitive::Char(c) => Value::String(c.to_string()),
            scale_value::Primitive::String(s) => Value::String(s),
            scale_value::Primitive::U128(n) => Value::String(n.to_string()),
            scale_value::Primitive::I128(n) => Value::String(n.to_string()),
            scale_value::Primitive::U256(n) => Value::String(format!("{:?}", n)),
            scale_value::Primitive::I256(n) => Value::String(format!("{:?}", n)),
        },
        scale_value::ValueDef::BitSequence(bits) => {
            // Convert bit sequence to hex string
            let bytes: Vec<u8> = bits
                .iter()
                .collect::<Vec<_>>()
                .chunks(8)
                .map(|chunk| {
                    chunk
                        .iter()
                        .enumerate()
                        .fold(0u8, |acc, (i, &bit)| acc | ((bit as u8) << i))
                })
                .collect();
            Value::String(format!("0x{}", hex::encode(bytes)))
        }
    }
}

// ================================================================================================
// Parachain Metadata Cache
// ================================================================================================

/// Cached metadata for a parachain, used for decoding non-XCM UMP messages.
#[derive(Clone)]
pub struct CachedParachainMetadata {
    /// The portable type registry from the parachain's runtime metadata
    registry: Arc<PortableRegistry>,
    /// Candidate type IDs that might represent UMP message payloads.
    /// Discovered by scanning the registry for types whose paths contain
    /// keywords like "BridgeMessage", "Call", "OutboundMessage", etc.
    candidate_type_ids: Vec<(u32, String)>,
}

/// Cache of parachain runtime metadata, keyed by para_id.
///
/// Fetches metadata lazily from configured parachain RPC endpoints and caches
/// the `PortableRegistry` for reuse across requests.
#[derive(Clone, Default)]
pub struct ParachainMetadataCache {
    /// Cached metadata per para_id
    cache: Arc<RwLock<HashMap<u32, CachedParachainMetadata>>>,
    /// Configured parachain RPC URLs, keyed by para_id
    rpc_urls: Arc<HashMap<u32, String>>,
}

impl ParachainMetadataCache {
    /// Create a new cache with the given parachain RPC URL mappings.
    pub fn new(rpc_urls: HashMap<u32, String>) -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            rpc_urls: Arc::new(rpc_urls),
        }
    }

    /// Check if we have a configured RPC URL for this parachain.
    pub fn has_rpc_url(&self, para_id: u32) -> bool {
        self.rpc_urls.contains_key(&para_id)
    }

    /// Get the cached metadata for a parachain, fetching it if not cached.
    /// Returns None if no RPC URL is configured or if fetching fails.
    pub async fn get_metadata(&self, para_id: u32) -> Option<CachedParachainMetadata> {
        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.get(&para_id) {
                return Some(cached.clone());
            }
        }

        // Fetch metadata from RPC
        let rpc_url = self.rpc_urls.get(&para_id)?;
        tracing::info!(
            para_id,
            rpc_url,
            "Fetching parachain metadata for UMP decode"
        );

        match self.fetch_and_cache_metadata(para_id, rpc_url).await {
            Ok(cached) => Some(cached),
            Err(e) => {
                tracing::warn!(para_id, error = %e, "Failed to fetch parachain metadata");
                None
            }
        }
    }

    /// Fetch metadata from the parachain RPC and cache it.
    async fn fetch_and_cache_metadata(
        &self,
        para_id: u32,
        rpc_url: &str,
    ) -> Result<CachedParachainMetadata, Box<dyn std::error::Error + Send + Sync>> {
        use parity_scale_codec::Decode;
        use subxt_rpcs::RpcClient;
        use subxt_rpcs::rpc_params;

        // Connect to the parachain RPC
        let rpc_client = RpcClient::from_insecure_url(rpc_url).await?;

        // Fetch raw metadata via state_getMetadata
        let metadata_hex: String = rpc_client
            .request("state_getMetadata", rpc_params![])
            .await?;
        let metadata_bytes = hex::decode(metadata_hex.trim_start_matches("0x"))?;

        // Decode the RuntimeMetadataPrefixed to extract the PortableRegistry
        let metadata_prefixed =
            frame_metadata::RuntimeMetadataPrefixed::decode(&mut &metadata_bytes[..])?;
        let registry = match metadata_prefixed.1 {
            frame_metadata::RuntimeMetadata::V14(ref m) => m.types.clone(),
            frame_metadata::RuntimeMetadata::V15(ref m) => m.types.clone(),
            _ => return Err(format!("Unsupported metadata version for para {}", para_id).into()),
        };

        // Scan registry for candidate UMP message types
        let candidate_type_ids = find_candidate_ump_types(&registry);
        tracing::info!(
            para_id,
            num_candidates = candidate_type_ids.len(),
            candidates = ?candidate_type_ids.iter().map(|(id, name)| format!("{name} (id={id})")).collect::<Vec<_>>(),
            "Discovered candidate UMP types in parachain metadata"
        );

        let cached = CachedParachainMetadata {
            registry: Arc::new(registry),
            candidate_type_ids,
        };

        // Store in cache
        let mut cache = self.cache.write().await;
        cache.insert(para_id, cached.clone());

        Ok(cached)
    }
}

/// Scan a PortableRegistry for types that are likely UMP message payloads.
///
/// Strategy: Look for types whose paths contain keywords associated with
/// bridge/cross-chain messaging. These are candidate types we'll try to
/// decode UMP bytes against.
fn find_candidate_ump_types(registry: &PortableRegistry) -> Vec<(u32, String)> {
    let keywords = [
        "BridgeMessage",
        "BridgeCall",
        "OutboundMessage",
        "UmpMessage",
        "ParachainAppCall",
        "SubstrateBridgeMessage",
        "BridgeTimepoint",
        "MessagePayload",
    ];

    let mut candidates = Vec::new();

    for ty in registry.types.iter() {
        let path = ty.ty.path.segments.join("::");
        let type_name = ty.ty.path.segments.last().map(|s| s.as_str()).unwrap_or("");

        // Check if this type's name matches any of our keywords
        if keywords.iter().any(|kw| type_name.contains(kw)) {
            candidates.push((ty.id, path.clone()));
        }

        // Also check for top-level RuntimeCall variants — these often contain
        // bridge-related calls as nested variants
        if type_name == "RuntimeCall" {
            candidates.push((ty.id, path.clone()));
        }
    }

    candidates
}

// ================================================================================================
// XCM / UMP Message Classification
// ================================================================================================

/// Known VersionedXcm SCALE discriminants from staging-xcm.
/// V0/V1 never existed. V2 was dropped in staging-xcm v21.
const VALID_XCM_DISCRIMINANTS: std::ops::RangeInclusive<u8> = 0x03..=0x05;

/// Check if a first byte is a known VersionedXcm SCALE discriminant.
fn is_likely_xcm(first_byte: u8) -> bool {
    VALID_XCM_DISCRIMINANTS.contains(&first_byte)
}

/// Return a human-readable label for an XCM version byte.
fn xcm_version_label(first_byte: u8) -> &'static str {
    match first_byte {
        0x03 => "V3",
        0x04 => "V4",
        0x05 => "V5",
        _ => "unknown",
    }
}

// ================================================================================================
// XCM Decoder
// ================================================================================================

/// Build a portable registry containing just the VersionedXcm type
fn build_xcm_registry() -> (PortableRegistry, u32) {
    let mut registry = scale_info::Registry::new();
    let type_id = registry.register_type(&scale_info::meta_type::<staging_xcm::VersionedXcm<()>>());
    (registry.into(), type_id.id)
}

/// Attempt to decode raw bytes using candidate types from a parachain's metadata.
///
/// Tries each candidate type ID in order. Returns the first successful decode
/// as a JSON value with the type path as the key.
fn try_parachain_metadata_decode(
    bytes: &[u8],
    metadata: &CachedParachainMetadata,
) -> Option<Value> {
    for (type_id, type_path) in &metadata.candidate_type_ids {
        match decode_as_type(&mut &bytes[..], *type_id, metadata.registry.as_ref()) {
            Ok(value) => {
                let json_value = scale_value_to_json(value, metadata.registry.as_ref());
                let mut result = serde_json::Map::new();
                // Use the last segment of the type path as the key (e.g., "BridgeMessage")
                let type_name = type_path
                    .rsplit("::")
                    .next()
                    .unwrap_or(type_path)
                    .to_lower_camel_case();
                result.insert(type_name, json_value);
                result.insert(
                    "decodedUsing".to_string(),
                    Value::String(format!(
                        "parachain metadata type: {type_path} (id={type_id})"
                    )),
                );
                tracing::debug!(
                    type_id,
                    type_path,
                    "Successfully decoded UMP message using parachain metadata"
                );
                return Some(Value::Object(result));
            }
            Err(_) => continue,
        }
    }
    None
}

/// Decode a hex-encoded UMP/DMP/HRMP message into a JSON value.
///
/// Uses a three-tier strategy:
/// 1. **XCM decode** — Try `VersionedXcm` (V3/V4/V5) via a local type registry
/// 2. **Parachain metadata decode** — If XCM fails and parachain metadata is
///    available, try candidate types from the parachain's `PortableRegistry`
/// 3. **Fallback** — Return raw hex with a `decodingNote` explaining the failure
fn decode_xcm_message(hex_str: &str, para_metadata: Option<&CachedParachainMetadata>) -> Value {
    let hex_clean = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    let Ok(bytes) = hex::decode(hex_clean) else {
        return Value::String(hex_str.to_string());
    };

    if bytes.is_empty() {
        return Value::String(hex_str.to_string());
    }

    let first_byte = bytes[0];

    // Tier 1: If first byte looks like XCM, try VersionedXcm decode
    if is_likely_xcm(first_byte) {
        let (registry, type_id) = build_xcm_registry();
        match decode_as_type(&mut &bytes[..], type_id, &registry) {
            Ok(value) => {
                return Value::Array(vec![scale_value_to_json(value, &registry)]);
            }
            Err(e) => {
                tracing::debug!(
                    first_byte = format!("0x{:02x}", first_byte),
                    version = xcm_version_label(first_byte),
                    error = %e,
                    "XCM first byte matches {} but SCALE decode failed",
                    xcm_version_label(first_byte)
                );
                // XCM-like first byte but decode failed — return structured error
                let mut result = serde_json::Map::new();
                result.insert("data".to_string(), Value::String(hex_str.to_string()));
                result.insert(
                    "decodingNote".to_string(),
                    Value::String(format!(
                        "First byte 0x{:02x} suggests XCM {} but decoding failed: {e}",
                        first_byte,
                        xcm_version_label(first_byte)
                    )),
                );
                return Value::Object(result);
            }
        }
    }

    // Tier 2: Non-XCM first byte — try parachain metadata if available
    if let Some(metadata) = para_metadata {
        if let Some(decoded) = try_parachain_metadata_decode(&bytes, metadata) {
            return decoded;
        }
        tracing::debug!(
            first_byte = format!("0x{:02x}", first_byte),
            "Parachain metadata available but no candidate type could decode this message"
        );
    }

    // Tier 3: Fallback — return raw hex with classification note
    let mut result = serde_json::Map::new();
    result.insert("data".to_string(), Value::String(hex_str.to_string()));
    let note = if first_byte <= 0x01 {
        format!(
            "First byte 0x{:02x} is not a known VersionedXcm discriminant (V3=0x03, V4=0x04, V5=0x05). \
             This is likely a custom bridge protocol message. {}",
            first_byte,
            if para_metadata.is_some() {
                "Parachain metadata was available but no matching type was found."
            } else {
                "No parachain RPC URL configured for metadata-based decoding."
            }
        )
    } else {
        format!(
            "First byte 0x{:02x} is not a known VersionedXcm discriminant (V3=0x03, V4=0x04, V5=0x05). {}",
            first_byte,
            if para_metadata.is_some() {
                "Parachain metadata was available but no matching type was found."
            } else {
                "No parachain RPC URL configured for metadata-based decoding."
            }
        )
    };
    result.insert("decodingNote".to_string(), Value::String(note));
    Value::Object(result)
}

/// Decodes XCM messages from block extrinsics.
///
/// Uses a three-tier decode strategy:
/// 1. Try `VersionedXcm` decode (handles V3/V4/V5)
/// 2. Try parachain metadata decode if RPC URL is configured for the sending para
/// 3. Fall back to raw hex with a `decodingNote`
pub struct XcmDecoder<'a> {
    chain_type: ChainType,
    extrinsics: &'a [ExtrinsicInfo],
    para_id_filter: Option<u32>,
    /// Optional cache of parachain metadata for decoding non-XCM UMP messages
    para_metadata_cache: Option<&'a ParachainMetadataCache>,
    /// Pre-fetched parachain metadata (resolved from cache before sync decode)
    /// Maps para_id -> CachedParachainMetadata
    resolved_metadata: HashMap<u32, CachedParachainMetadata>,
}

impl<'a> XcmDecoder<'a> {
    pub fn new(
        chain_type: ChainType,
        extrinsics: &'a [ExtrinsicInfo],
        para_id_filter: Option<u32>,
    ) -> Self {
        Self {
            chain_type,
            extrinsics,
            para_id_filter,
            para_metadata_cache: None,
            resolved_metadata: HashMap::new(),
        }
    }

    /// Set the parachain metadata cache for non-XCM UMP decode.
    pub fn with_metadata_cache(mut self, cache: &'a ParachainMetadataCache) -> Self {
        self.para_metadata_cache = Some(cache);
        self
    }

    /// Decode XCM messages from the extrinsics.
    ///
    /// This is async because it may need to fetch parachain metadata from
    /// remote RPC endpoints on the first call for a given para_id.
    pub async fn decode(&mut self) -> XcmMessages {
        // Pre-fetch metadata for all para_ids we'll encounter
        if let Some(cache) = self.para_metadata_cache {
            let para_ids = self.collect_para_ids();
            for para_id in para_ids {
                if cache.has_rpc_url(para_id)
                    && let Some(metadata) = cache.get_metadata(para_id).await
                {
                    self.resolved_metadata.insert(para_id, metadata);
                }
            }
        }

        match self.chain_type {
            ChainType::Relay => self.decode_relay_messages(),
            ChainType::Parachain | ChainType::AssetHub | ChainType::Coretime => {
                self.decode_parachain_messages()
            }
        }
    }

    /// Collect all unique para_ids that appear in the extrinsics.
    fn collect_para_ids(&self) -> Vec<u32> {
        let mut para_ids = Vec::new();

        for extrinsic in self.extrinsics {
            if extrinsic.method.pallet == "paraInherent"
                && extrinsic.method.method == "enter"
                && let Some(data) = extrinsic.args.get("data")
                && let Some(backed_candidates) =
                    data.get("backedCandidates").and_then(|v| v.as_array())
            {
                for candidate in backed_candidates {
                    if let Some(para_id) = candidate
                        .get("candidate")
                        .and_then(|c| c.get("descriptor"))
                        .and_then(|d| d.get("paraId"))
                        .and_then(|p| p.as_str())
                        .and_then(|s| s.parse::<u32>().ok())
                        && !para_ids.contains(&para_id)
                    {
                        para_ids.push(para_id);
                    }
                }
            }
        }

        para_ids
    }

    /// Get cached parachain metadata for a given para_id (sync, pre-resolved).
    fn get_para_metadata(&self, para_id: u32) -> Option<&CachedParachainMetadata> {
        self.resolved_metadata.get(&para_id)
    }

    /// Decode XCM messages from relay chain extrinsics.
    /// Looks for `paraInherent.enter` and extracts upward/horizontal messages from backedCandidates.
    fn decode_relay_messages(&self) -> XcmMessages {
        let mut messages = XcmMessages::default();

        for extrinsic in self.extrinsics {
            if extrinsic.method.pallet != "paraInherent" || extrinsic.method.method != "enter" {
                continue;
            }

            let Some(data) = extrinsic.args.get("data") else {
                continue;
            };

            let Some(backed_candidates) = data.get("backedCandidates").and_then(|v| v.as_array())
            else {
                continue;
            };

            for candidate in backed_candidates {
                let Some(candidate_obj) = candidate.get("candidate") else {
                    continue;
                };

                let para_id = candidate_obj
                    .get("descriptor")
                    .and_then(|d| d.get("paraId"))
                    .and_then(|p| p.as_str())
                    .unwrap_or("0");

                if self
                    .para_id_filter
                    .is_some_and(|filter| para_id != filter.to_string())
                {
                    continue;
                }

                let Some(commitments) = candidate_obj.get("commitments") else {
                    continue;
                };

                // Look up parachain metadata for this para_id (for non-XCM decode fallback)
                let para_id_u32 = para_id.parse::<u32>().unwrap_or(0);
                let para_meta = self.get_para_metadata(para_id_u32);

                // Extract upward messages
                // upwardMessages can be either:
                // 1. An array of hex strings (when there are multiple messages or empty)
                // 2. A single hex string (when there's one message - this is how subxt decodes it)
                if let Some(upward_value) = commitments.get("upwardMessages") {
                    if let Some(msg_data) = upward_value.as_str() {
                        // Single hex string - decode it directly
                        if !msg_data.is_empty() && msg_data != "0x" {
                            messages.upward_messages.push(UpwardMessage {
                                origin_para_id: para_id.to_string(),
                                data: decode_xcm_message(msg_data, para_meta),
                            });
                        }
                    } else if let Some(upward_msgs) = upward_value.as_array() {
                        // Array of hex strings
                        for msg in upward_msgs {
                            if let Some(msg_data) = msg.as_str()
                                && !msg_data.is_empty()
                            {
                                messages.upward_messages.push(UpwardMessage {
                                    origin_para_id: para_id.to_string(),
                                    data: decode_xcm_message(msg_data, para_meta),
                                });
                            }
                        }
                    }
                }

                // Extract horizontal messages
                if let Some(horizontal_msgs) = commitments
                    .get("horizontalMessages")
                    .and_then(|v| v.as_array())
                {
                    for msg in horizontal_msgs {
                        let recipient =
                            msg.get("recipient").and_then(|r| r.as_str()).unwrap_or("0");
                        let msg_data = msg.get("data").and_then(|d| d.as_str()).unwrap_or("");

                        if !msg_data.is_empty() {
                            messages.horizontal_messages.push(HorizontalMessage {
                                origin_para_id: para_id.to_string(),
                                destination_para_id: Some(recipient.to_string()),
                                sent_at: None,
                                data: decode_xcm_message(msg_data, para_meta),
                            });
                        }
                    }
                }
            }
        }

        messages
    }

    /// Decode XCM messages from parachain extrinsics.
    /// Looks for `parachainSystem.setValidationData` and extracts downward/horizontal messages.
    fn decode_parachain_messages(&self) -> XcmMessages {
        let mut messages = XcmMessages::default();

        for extrinsic in self.extrinsics {
            if extrinsic.method.pallet != "parachainSystem"
                || extrinsic.method.method != "setValidationData"
            {
                continue;
            }

            let Some(data) = extrinsic.args.get("data") else {
                continue;
            };

            let Some(inbound_data) = data.get("inbound_messages_data") else {
                continue;
            };

            // Extract downward messages
            if let Some(downward) = inbound_data.get("downwardMessages")
                && let Some(full_msgs) = downward.get("fullMessages").and_then(|v| v.as_array())
            {
                for msg in full_msgs {
                    let sent_at = msg
                        .get("sentAt")
                        .and_then(|v| v.as_str())
                        .unwrap_or("0")
                        .to_string();
                    let msg_hex = msg
                        .get("msg")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    if !msg_hex.is_empty() {
                        messages.downward_messages.push(DownwardMessage {
                            sent_at,
                            msg: msg_hex.clone(),
                            data: decode_xcm_message(&msg_hex, None),
                        });
                    }
                }
            }

            // Extract horizontal messages
            if let Some(horizontal) = inbound_data.get("horizontalMessages")
                && let Some(full_msgs) = horizontal.get("fullMessages").and_then(|v| v.as_array())
            {
                for msg in full_msgs {
                    let sent_at = msg
                        .get("sentAt")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let origin_para_id = msg
                        .get("originParaId")
                        .and_then(|v| v.as_str())
                        .unwrap_or("0")
                        .to_string();
                    let msg_data = msg
                        .get("data")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    // Apply paraId filter if specified
                    if self
                        .para_id_filter
                        .is_some_and(|filter| origin_para_id != filter.to_string())
                    {
                        continue;
                    }

                    if !msg_data.is_empty() {
                        messages.horizontal_messages.push(HorizontalMessage {
                            origin_para_id,
                            destination_para_id: None, // Not available for parachain perspective
                            sent_at,
                            data: decode_xcm_message(&msg_data, None),
                        });
                    }
                }
            }
        }

        messages
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parity_scale_codec::Encode;
    use staging_xcm::VersionedXcm;
    use staging_xcm::v4 as xcm_v4;
    use staging_xcm::v5 as xcm_v5;

    /// Helper: SCALE-encode a VersionedXcm and return it as a "0x"-prefixed hex string.
    fn encode_versioned_xcm(msg: VersionedXcm<()>) -> String {
        let bytes = msg.encode();
        format!("0x{}", hex::encode(bytes))
    }

    #[test]
    fn test_decode_v4_xcm_withdraw_and_deposit() {
        // Build a V4 XCM: WithdrawAsset + BuyExecution + DepositAsset
        let xcm = VersionedXcm::V4(xcm_v4::Xcm(vec![
            xcm_v4::Instruction::WithdrawAsset(
                vec![xcm_v4::Asset {
                    id: xcm_v4::AssetId(xcm_v4::Location::here()),
                    fun: xcm_v4::Fungibility::Fungible(1_000_000_000),
                }]
                .into(),
            ),
            xcm_v4::Instruction::BuyExecution {
                fees: xcm_v4::Asset {
                    id: xcm_v4::AssetId(xcm_v4::Location::here()),
                    fun: xcm_v4::Fungibility::Fungible(500_000_000),
                },
                weight_limit: xcm_v4::WeightLimit::Unlimited,
            },
            xcm_v4::Instruction::DepositAsset {
                assets: xcm_v4::AssetFilter::Wild(xcm_v4::WildAsset::AllCounted(1)),
                beneficiary: xcm_v4::Location {
                    parents: 0,
                    interior: xcm_v4::Junctions::X1(
                        [xcm_v4::Junction::AccountId32 {
                            network: None,
                            id: [1u8; 32],
                        }]
                        .into(),
                    ),
                },
            },
        ]));

        let hex_str = encode_versioned_xcm(xcm);
        let result = decode_xcm_message(&hex_str, None);

        // Should be an array with one element containing "v4" key
        let arr = result.as_array().expect("result should be an array");
        assert_eq!(arr.len(), 1, "should have exactly one decoded message");
        let msg = arr[0].as_object().expect("message should be an object");
        assert!(
            msg.contains_key("v4"),
            "should contain 'v4' key, got keys: {:?}",
            msg.keys().collect::<Vec<_>>()
        );

        // The V4 value should be an array of 3 instructions
        let instructions = msg["v4"]
            .as_array()
            .expect("v4 should contain an array of instructions");
        assert_eq!(instructions.len(), 3, "should have 3 instructions");

        // Check instruction names
        let first = instructions[0].as_object().unwrap();
        assert!(
            first.contains_key("withdrawAsset"),
            "first instruction should be withdrawAsset"
        );
        let second = instructions[1].as_object().unwrap();
        assert!(
            second.contains_key("buyExecution"),
            "second instruction should be buyExecution"
        );
        let third = instructions[2].as_object().unwrap();
        assert!(
            third.contains_key("depositAsset"),
            "third instruction should be depositAsset"
        );
    }

    #[test]
    fn test_decode_v5_xcm_withdraw_and_deposit() {
        // Build a V5 XCM: WithdrawAsset + BuyExecution + DepositAsset
        let xcm = VersionedXcm::V5(xcm_v5::Xcm(vec![
            xcm_v5::Instruction::WithdrawAsset(
                vec![xcm_v5::Asset {
                    id: xcm_v5::AssetId(xcm_v5::Location::here()),
                    fun: xcm_v5::Fungibility::Fungible(1_000_000_000),
                }]
                .into(),
            ),
            xcm_v5::Instruction::BuyExecution {
                fees: xcm_v5::Asset {
                    id: xcm_v5::AssetId(xcm_v5::Location::here()),
                    fun: xcm_v5::Fungibility::Fungible(500_000_000),
                },
                weight_limit: xcm_v5::WeightLimit::Unlimited,
            },
            xcm_v5::Instruction::DepositAsset {
                assets: xcm_v5::AssetFilter::Wild(xcm_v5::WildAsset::AllCounted(1)),
                beneficiary: xcm_v5::Location {
                    parents: 0,
                    interior: xcm_v5::Junctions::X1(
                        [xcm_v5::Junction::AccountId32 {
                            network: None,
                            id: [1u8; 32],
                        }]
                        .into(),
                    ),
                },
            },
        ]));

        let hex_str = encode_versioned_xcm(xcm);
        let result = decode_xcm_message(&hex_str, None);

        // Should be an array with one element containing "v5" key
        let arr = result.as_array().expect("result should be an array");
        assert_eq!(arr.len(), 1, "should have exactly one decoded message");
        let msg = arr[0].as_object().expect("message should be an object");
        assert!(
            msg.contains_key("v5"),
            "should contain 'v5' key, got keys: {:?}",
            msg.keys().collect::<Vec<_>>()
        );

        // The V5 value should be an array of 3 instructions
        let instructions = msg["v5"]
            .as_array()
            .expect("v5 should contain an array of instructions");
        assert_eq!(instructions.len(), 3, "should have 3 instructions");

        // Check instruction names
        let first = instructions[0].as_object().unwrap();
        assert!(
            first.contains_key("withdrawAsset"),
            "first instruction should be withdrawAsset"
        );
        let second = instructions[1].as_object().unwrap();
        assert!(
            second.contains_key("buyExecution"),
            "second instruction should be buyExecution"
        );
        let third = instructions[2].as_object().unwrap();
        assert!(
            third.contains_key("depositAsset"),
            "third instruction should be depositAsset"
        );
    }

    #[test]
    fn test_decode_v5_xcm_with_set_topic() {
        // V5 XCM with ClearOrigin and SetTopic (common pattern)
        let topic = [0xABu8; 32];
        let xcm = VersionedXcm::V5(xcm_v5::Xcm(vec![
            xcm_v5::Instruction::ClearOrigin,
            xcm_v5::Instruction::SetTopic(topic),
        ]));

        let hex_str = encode_versioned_xcm(xcm);
        let result = decode_xcm_message(&hex_str, None);

        let arr = result.as_array().unwrap();
        let msg = arr[0].as_object().unwrap();
        assert!(msg.contains_key("v5"), "should decode as v5");

        let instructions = msg["v5"].as_array().unwrap();
        assert_eq!(instructions.len(), 2);

        let first = instructions[0].as_object().unwrap();
        assert!(first.contains_key("clearOrigin"));
        let second = instructions[1].as_object().unwrap();
        assert!(second.contains_key("setTopic"));
    }

    #[test]
    fn test_decode_v5_xcm_reserve_asset_deposited() {
        // V5 XCM: ReserveAssetDeposited + ClearOrigin + BuyExecution + DepositAsset
        // This is a common pattern for cross-chain transfers
        let xcm = VersionedXcm::V5(xcm_v5::Xcm(vec![
            xcm_v5::Instruction::ReserveAssetDeposited(
                vec![xcm_v5::Asset {
                    id: xcm_v5::AssetId(xcm_v5::Location {
                        parents: 1,
                        interior: xcm_v5::Junctions::Here,
                    }),
                    fun: xcm_v5::Fungibility::Fungible(5_000_000_000),
                }]
                .into(),
            ),
            xcm_v5::Instruction::ClearOrigin,
            xcm_v5::Instruction::BuyExecution {
                fees: xcm_v5::Asset {
                    id: xcm_v5::AssetId(xcm_v5::Location {
                        parents: 1,
                        interior: xcm_v5::Junctions::Here,
                    }),
                    fun: xcm_v5::Fungibility::Fungible(500_000_000),
                },
                weight_limit: xcm_v5::WeightLimit::Unlimited,
            },
            xcm_v5::Instruction::DepositAsset {
                assets: xcm_v5::AssetFilter::Wild(xcm_v5::WildAsset::All),
                beneficiary: xcm_v5::Location {
                    parents: 0,
                    interior: xcm_v5::Junctions::X1(
                        [xcm_v5::Junction::AccountId32 {
                            network: None,
                            id: [2u8; 32],
                        }]
                        .into(),
                    ),
                },
            },
        ]));

        let hex_str = encode_versioned_xcm(xcm);
        let result = decode_xcm_message(&hex_str, None);

        let arr = result.as_array().unwrap();
        let msg = arr[0].as_object().unwrap();
        assert!(msg.contains_key("v5"));

        let instructions = msg["v5"].as_array().unwrap();
        assert_eq!(instructions.len(), 4);

        assert!(
            instructions[0]
                .as_object()
                .unwrap()
                .contains_key("reserveAssetDeposited")
        );
        assert!(
            instructions[1]
                .as_object()
                .unwrap()
                .contains_key("clearOrigin")
        );
        assert!(
            instructions[2]
                .as_object()
                .unwrap()
                .contains_key("buyExecution")
        );
        assert!(
            instructions[3]
                .as_object()
                .unwrap()
                .contains_key("depositAsset")
        );
    }

    #[test]
    fn test_decode_invalid_hex_returns_raw_string() {
        // Invalid hex should return the raw string
        let result = decode_xcm_message("not_valid_hex", None);
        assert_eq!(result, Value::String("not_valid_hex".to_string()));
    }

    #[test]
    fn test_decode_malformed_xcm_returns_structured_fallback() {
        // Valid hex but not a valid XCM message (0xde is not a known XCM discriminant)
        // Should return a structured fallback with data and decodingNote
        let result = decode_xcm_message("0xdeadbeef", None);
        let obj = result
            .as_object()
            .expect("should return a JSON object with data + decodingNote");
        assert_eq!(
            obj.get("data").and_then(|v| v.as_str()),
            Some("0xdeadbeef"),
            "should contain original hex in 'data' field"
        );
        assert!(
            obj.contains_key("decodingNote"),
            "should contain a 'decodingNote' explaining the failure"
        );
    }

    // ========================================================================================
    // Tier 2 & Tier 3 tests — parachain metadata decode path
    // ========================================================================================

    /// Build a fake CachedParachainMetadata with a known SCALE type (u64)
    /// so that we can test the Tier 2 code path without a real parachain RPC.
    fn build_test_metadata_with_u64() -> CachedParachainMetadata {
        // Register a simple u64 type into a portable registry
        let mut registry = scale_info::Registry::new();
        let type_id = registry.register_type(&scale_info::meta_type::<u64>());
        let portable: PortableRegistry = registry.into();
        CachedParachainMetadata {
            registry: Arc::new(portable),
            candidate_type_ids: vec![(type_id.id, "test::TestMessage".to_string())],
        }
    }

    /// Build a fake CachedParachainMetadata with RuntimeCall-like name but an
    /// empty candidate list (simulating metadata fetched but no matching types).
    fn build_test_metadata_empty_candidates() -> CachedParachainMetadata {
        let registry = scale_info::Registry::new();
        let portable: PortableRegistry = registry.into();
        CachedParachainMetadata {
            registry: Arc::new(portable),
            candidate_type_ids: vec![],
        }
    }

    #[test]
    fn test_tier2_successful_decode_with_parachain_metadata() {
        // Construct a SCALE-encoded u64 value (little-endian)
        let value: u64 = 42;
        let encoded = value.encode();
        // First byte of u64 encoding will be 0x2a (42), which is NOT an XCM discriminant
        let hex_str = format!("0x{}", hex::encode(&encoded));

        let metadata = build_test_metadata_with_u64();
        let result = decode_xcm_message(&hex_str, Some(&metadata));

        // Tier 2 should succeed: the message should be decoded using the candidate type
        let obj = result
            .as_object()
            .expect("Tier 2 success should return a JSON object");
        assert!(
            obj.contains_key("testMessage"),
            "Should contain key derived from type path 'test::TestMessage' → 'testMessage', got keys: {:?}",
            obj.keys().collect::<Vec<_>>()
        );
        assert!(
            obj.contains_key("decodedUsing"),
            "Should contain 'decodedUsing' field"
        );
        let decoded_using = obj["decodedUsing"].as_str().unwrap();
        assert!(
            decoded_using.contains("test::TestMessage"),
            "decodedUsing should reference the type path"
        );
    }

    #[test]
    fn test_tier2_failure_falls_to_tier3_with_metadata_note() {
        // Protobuf-like bytes that won't SCALE-decode as u64 (too short for u64)
        let hex_str = "0x0198";

        let metadata = build_test_metadata_with_u64();
        let result = decode_xcm_message(hex_str, Some(&metadata));

        // Should fall through Tier 2 to Tier 3 with "metadata was available" note
        let obj = result
            .as_object()
            .expect("should return a JSON object with data + decodingNote");
        assert_eq!(
            obj.get("data").and_then(|v| v.as_str()),
            Some("0x0198"),
            "should contain original hex"
        );
        let note = obj["decodingNote"]
            .as_str()
            .expect("should have decodingNote");
        assert!(
            note.contains("Parachain metadata was available but no matching type was found"),
            "decodingNote should mention metadata was available, got: {note}"
        );
    }

    #[test]
    fn test_tier2_empty_candidates_falls_to_tier3() {
        // Even with metadata provided, if there are no candidate types, should fall to Tier 3
        let hex_str = "0x0198aabbccdd";

        let metadata = build_test_metadata_empty_candidates();
        let result = decode_xcm_message(hex_str, Some(&metadata));

        let obj = result.as_object().expect("should return fallback object");
        let note = obj["decodingNote"]
            .as_str()
            .expect("should have decodingNote");
        assert!(
            note.contains("Parachain metadata was available but no matching type was found"),
            "should indicate metadata was available: {note}"
        );
    }

    #[test]
    fn test_tier3_no_metadata_note() {
        // Non-XCM bytes with NO parachain metadata → Tier 3 with "no RPC configured" note
        let hex_str = "0x0198aabbccdd";

        let result = decode_xcm_message(hex_str, None);

        let obj = result.as_object().expect("should return fallback object");
        let note = obj["decodingNote"]
            .as_str()
            .expect("should have decodingNote");
        assert!(
            note.contains("No parachain RPC URL configured for metadata-based decoding"),
            "should indicate no RPC configured: {note}"
        );
    }

    #[test]
    fn test_tier3_bridge_protocol_pattern() {
        // Real-world pattern from para 3428: starts with 0x01 0x98 (bridge protocol)
        let hex_str =
            "0x01980024080112207b213f6092fd22eb2493a9e7889927f003d881618d0d0984b5555db7ecb2f9";

        // Without metadata
        let result_no_meta = decode_xcm_message(hex_str, None);
        let obj = result_no_meta.as_object().unwrap();
        let note = obj["decodingNote"].as_str().unwrap();
        assert!(
            note.contains("custom bridge protocol message"),
            "0x01 prefix should be classified as bridge protocol: {note}"
        );
        assert!(
            note.contains("No parachain RPC URL configured"),
            "without metadata should say no RPC configured: {note}"
        );

        // With metadata but empty candidates (simulating Tier 2 attempt with no matching types)
        // Note: We use empty candidates here because a simple u64 type would accidentally
        // "succeed" by consuming just the first 8 bytes of the protobuf data.
        let metadata = build_test_metadata_empty_candidates();
        let result_with_meta = decode_xcm_message(hex_str, Some(&metadata));
        let obj2 = result_with_meta.as_object().unwrap();
        let note2 = obj2["decodingNote"].as_str().unwrap();
        assert!(
            note2.contains("Parachain metadata was available but no matching type was found"),
            "with metadata should say metadata was available: {note2}"
        );
    }

    #[test]
    fn test_xcm_discriminant_that_fails_decode_returns_structured_error() {
        // 0x04 is V4 discriminant but followed by garbage bytes → Tier 1 fails with structured error
        let hex_str = "0x04deadbeefcafe";
        let result = decode_xcm_message(hex_str, None);

        let obj = result.as_object().expect("should return structured error");
        assert_eq!(obj.get("data").and_then(|v| v.as_str()), Some(hex_str));
        let note = obj["decodingNote"].as_str().unwrap();
        assert!(
            note.contains("suggests XCM V4 but decoding failed"),
            "should explain V4 decode failure: {note}"
        );
    }

    #[test]
    fn test_is_likely_xcm() {
        // 0x03=V3, 0x04=V4, 0x05=V5 are XCM discriminants
        assert!(is_likely_xcm(0x03));
        assert!(is_likely_xcm(0x04));
        assert!(is_likely_xcm(0x05));
        // Everything else is not
        assert!(!is_likely_xcm(0x00));
        assert!(!is_likely_xcm(0x01));
        assert!(!is_likely_xcm(0x02));
        assert!(!is_likely_xcm(0x06));
        assert!(!is_likely_xcm(0xFF));
    }

    #[test]
    fn test_find_candidate_ump_types() {
        // Create a registry with a type that has "RuntimeCall" in its path
        // The find_candidate_ump_types function checks the last path segment
        let registry = scale_info::Registry::new();
        let portable: PortableRegistry = registry.into();
        // With an empty registry, should find no candidates
        let candidates = find_candidate_ump_types(&portable);
        assert!(
            candidates.is_empty(),
            "empty registry should have no candidates"
        );
    }

    #[test]
    fn test_v4_and_v5_produce_different_encodings() {
        // Same logical message encoded as V4 vs V5 should produce different hex
        // (because the version discriminant byte differs: V4=0x04, V5=0x05)
        let instructions_v4 = vec![xcm_v4::Instruction::ClearOrigin];
        let instructions_v5 = vec![xcm_v5::Instruction::ClearOrigin];

        let v4_hex = encode_versioned_xcm(VersionedXcm::V4(xcm_v4::Xcm(instructions_v4)));
        let v5_hex = encode_versioned_xcm(VersionedXcm::V5(xcm_v5::Xcm(instructions_v5)));

        assert_ne!(v4_hex, v5_hex, "V4 and V5 should have different encodings");

        // V4 should start with "0x04", V5 with "0x05"
        assert!(
            v4_hex.starts_with("0x04"),
            "V4 should start with 0x04, got {}",
            &v4_hex[..6]
        );
        assert!(
            v5_hex.starts_with("0x05"),
            "V5 should start with 0x05, got {}",
            &v5_hex[..6]
        );

        // Both should decode successfully
        let r4 = decode_xcm_message(&v4_hex, None);
        let r5 = decode_xcm_message(&v5_hex, None);

        let r4_obj = r4.as_array().unwrap()[0].as_object().unwrap();
        let r5_obj = r5.as_array().unwrap()[0].as_object().unwrap();
        assert!(r4_obj.contains_key("v4"));
        assert!(r5_obj.contains_key("v5"));
    }
}
