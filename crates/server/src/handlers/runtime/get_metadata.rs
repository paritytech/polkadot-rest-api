//! Handler for `/runtime/metadata` endpoint.
//!
//! Returns the decoded runtime metadata in JSON format matching sidecar's output.

use crate::state::AppState;
use crate::utils;
use axum::{Json, extract::Path, extract::State, http::StatusCode, response::IntoResponse};
use frame_metadata::v14 as v14_types;
use frame_metadata::v15 as v15_types;
use frame_metadata::{RuntimeMetadata, RuntimeMetadataPrefixed};
use lazy_static::lazy_static;
use parity_scale_codec::Decode;
use regex::Regex;
use scale_info::{PortableRegistry, form::PortableForm};
use serde::Serialize;
use serde_json::{Value, json};
use subxt_rpcs::rpc_params;
use thiserror::Error;

lazy_static! {
    pub static ref VERSION_REGEX: Regex =
        Regex::new(r"^[vV](\d+)$").expect("VERSION_REGEX is a valid regex pattern");
}

#[derive(Debug, Error)]
pub enum GetMetadataError {
    #[error("Invalid block parameter")]
    InvalidBlockParam(#[from] crate::utils::BlockIdParseError),

    #[error("Failed to get metadata from RPC")]
    RpcFailed(#[source] subxt_rpcs::Error),

    #[error("Failed to decode metadata hex")]
    HexDecodeFailed(#[source] hex::FromHexError),

    #[error("Failed to decode metadata SCALE")]
    ScaleDecodeFailed(#[source] parity_scale_codec::Error),

    #[error("Metadata bytes too short")]
    MetadataTooShort,

    #[error("Unsupported metadata version")]
    UnsupportedVersion,

    #[error(
        "Invalid version format: {0}. Expected format 'vX' where X is a number (e.g., 'v14', 'v15')"
    )]
    InvalidVersionFormat(String),

    #[error("Metadata version {0} is not available")]
    VersionNotAvailable(u32),

    #[error("Function 'metadata.metadataVersions()' is not available at this block height")]
    MetadataVersionsNotAvailable,

    #[error("Service temporarily unavailable: {0}")]
    ServiceUnavailable(String),

    #[error("Block not found: {0}")]
    BlockNotFound(String),
}

impl IntoResponse for GetMetadataError {
    fn into_response(self) -> axum::response::Response {
        let (status, message) = match &self {
            GetMetadataError::InvalidBlockParam(_)
            | GetMetadataError::InvalidVersionFormat(_)
            | GetMetadataError::VersionNotAvailable(_)
            | GetMetadataError::MetadataVersionsNotAvailable
            | GetMetadataError::BlockNotFound(_) => (StatusCode::BAD_REQUEST, self.to_string()),
            GetMetadataError::ServiceUnavailable(_) => {
                (StatusCode::SERVICE_UNAVAILABLE, self.to_string())
            }
            GetMetadataError::RpcFailed(err) => utils::rpc_error_to_status(err),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, self.to_string()),
        };

        let body = Json(json!({
            "error": message,
        }));

        (status, body).into_response()
    }
}

/// Resolve a block identifier to just the block hash string.
/// This is optimized for handlers that only need the hash (not the number).
async fn resolve_block_hash(
    state: &AppState,
    at: Option<&str>,
) -> Result<String, GetMetadataError> {
    match at {
        None => {
            // Get latest finalized block hash
            let hash = state
                .legacy_rpc
                .chain_get_finalized_head()
                .await
                .map_err(GetMetadataError::RpcFailed)?;
            Ok(format!("{:#x}", hash))
        }
        Some(at_str) => {
            let block_id = at_str.parse::<crate::utils::BlockId>()?;
            match block_id {
                crate::utils::BlockId::Hash(hash) => {
                    // Already have the hash, just format it
                    Ok(format!("{:#x}", hash))
                }
                crate::utils::BlockId::Number(number) => {
                    // Need to fetch hash from number
                    let hash = state
                        .get_block_hash_at_number(number)
                        .await
                        .map_err(GetMetadataError::RpcFailed)?
                        .ok_or_else(|| {
                            GetMetadataError::BlockNotFound(format!(
                                "Block at height {} not found",
                                number
                            ))
                        })?;
                    Ok(hash)
                }
            }
        }
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct AtBlockParam {
    pub at: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeMetadataResponse {
    pub magic_number: String,
    pub metadata: Value,
}

#[utoipa::path(
    get,
    path = "/v1/runtime/metadata",
    tag = "runtime",
    summary = "Runtime metadata",
    description = "Returns the decoded runtime metadata in JSON format.",
    params(
        ("at" = Option<String>, Query, description = "Block hash or number to query at")
    ),
    responses(
        (status = 200, description = "Runtime metadata", body = Object),
        (status = 400, description = "Invalid block parameter"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn runtime_metadata(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<AtBlockParam>,
) -> Result<Json<RuntimeMetadataResponse>, GetMetadataError> {
    // Get block hash - only fetch what we need (saves RPC call when hash provided)
    let block_hash = resolve_block_hash(&state, params.at.as_deref()).await?;

    let metadata_hex: String = state
        .rpc_client
        .request("state_getMetadata", rpc_params![&block_hash])
        .await
        .map_err(GetMetadataError::RpcFailed)?;

    let hex_str = metadata_hex.strip_prefix("0x").unwrap_or(&metadata_hex);
    let metadata_bytes = hex::decode(hex_str).map_err(GetMetadataError::HexDecodeFailed)?;

    if metadata_bytes.len() < 4 {
        return Err(GetMetadataError::MetadataTooShort);
    }

    // Magic number is the first 4 bytes as little-endian u32
    let magic_number = u32::from_le_bytes(
        metadata_bytes[0..4]
            .try_into()
            .map_err(|_| GetMetadataError::MetadataTooShort)?,
    );

    let metadata_prefixed = RuntimeMetadataPrefixed::decode(&mut &metadata_bytes[..])
        .map_err(GetMetadataError::ScaleDecodeFailed)?;

    let metadata = convert_metadata(&metadata_prefixed.1)?;

    Ok(Json(RuntimeMetadataResponse {
        magic_number: magic_number.to_string(),
        metadata,
    }))
}

#[utoipa::path(
    get,
    path = "/v1/runtime/metadata/versions",
    tag = "runtime",
    summary = "Available metadata versions",
    description = "Returns the available metadata versions at a given block.",
    params(
        ("at" = Option<String>, Query, description = "Block hash or number to query at")
    ),
    responses(
        (status = 200, description = "List of available metadata versions", body = Vec<String>),
        (status = 400, description = "Invalid block parameter"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn runtime_metadata_versions(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<AtBlockParam>,
) -> Result<Json<Vec<String>>, GetMetadataError> {
    // Get block hash - only fetch what we need (saves RPC call when hash provided)
    let block_hash = resolve_block_hash(&state, params.at.as_deref()).await?;

    // Call state_call with Metadata_metadata_versions
    // The call takes no parameters, so we just encode empty bytes
    let call_data = "0x".to_string();

    let result: String = state
        .rpc_client
        .request(
            "state_call",
            rpc_params!["Metadata_metadata_versions", &call_data, &block_hash],
        )
        .await
        .map_err(|e| {
            // Check if this is a "method not found" error (runtime API not available)
            let err_str = e.to_string().to_lowercase();
            if err_str.contains("not found") || err_str.contains("does not exist") {
                GetMetadataError::MetadataVersionsNotAvailable
            } else {
                GetMetadataError::RpcFailed(e)
            }
        })?;

    // Decode the result - it's a Vec<u32>
    let hex_str = result.strip_prefix("0x").unwrap_or(&result);
    let bytes = hex::decode(hex_str).map_err(GetMetadataError::HexDecodeFailed)?;

    let versions: Vec<u32> =
        Vec::<u32>::decode(&mut &bytes[..]).map_err(GetMetadataError::ScaleDecodeFailed)?;

    // Convert to "vX" format matching Sidecar output
    let version_strings: Vec<String> = versions.iter().map(|v| format!("{}", v)).collect();

    Ok(Json(version_strings))
}

#[utoipa::path(
    get,
    path = "/v1/runtime/metadata/{version}",
    tag = "runtime",
    summary = "Runtime metadata by version",
    description = "Returns the metadata at a specific version. The version parameter should be in 'vX' format (e.g., 'v14', 'v15').",
    params(
        ("version" = String, Path, description = "Metadata version (e.g., 'v14', 'v15')"),
        ("at" = Option<String>, Query, description = "Block hash or number to query at")
    ),
    responses(
        (status = 200, description = "Runtime metadata at specified version", body = Object),
        (status = 400, description = "Invalid version format or block parameter"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn runtime_metadata_versioned(
    State(state): State<AppState>,
    Path(version): Path<String>,
    axum::extract::Query(params): axum::extract::Query<AtBlockParam>,
) -> Result<Json<RuntimeMetadataResponse>, GetMetadataError> {
    let version_num: u32 = match VERSION_REGEX.captures(&version) {
        Some(caps) => {
            // Safe unwrap: regex guarantees capture group 1 exists when there's a match
            caps.get(1)
                .expect("regex capture group 1 must exist")
                .as_str()
                .parse()
                .map_err(|_| GetMetadataError::InvalidVersionFormat(version.clone()))?
        }
        None => return Err(GetMetadataError::InvalidVersionFormat(version.clone())),
    };

    // Get block hash - only fetch what we need (saves RPC call when hash provided)
    let block_hash = resolve_block_hash(&state, params.at.as_deref()).await?;

    // First, check if the version is available
    let versions_call_data = "0x".to_string();
    let versions_result: String = state
        .rpc_client
        .request(
            "state_call",
            rpc_params![
                "Metadata_metadata_versions",
                &versions_call_data,
                &block_hash
            ],
        )
        .await
        .map_err(|e| {
            let err_str = e.to_string().to_lowercase();
            if err_str.contains("not found") || err_str.contains("does not exist") {
                GetMetadataError::MetadataVersionsNotAvailable
            } else {
                GetMetadataError::RpcFailed(e)
            }
        })?;

    let versions_hex = versions_result
        .strip_prefix("0x")
        .unwrap_or(&versions_result);
    let versions_bytes = hex::decode(versions_hex).map_err(GetMetadataError::HexDecodeFailed)?;
    let available_versions: Vec<u32> = Vec::<u32>::decode(&mut &versions_bytes[..])
        .map_err(GetMetadataError::ScaleDecodeFailed)?;

    if !available_versions.contains(&version_num) {
        return Err(GetMetadataError::VersionNotAvailable(version_num));
    }

    // Call state_call with Metadata_metadata_at_version
    // The parameter is a u32 encoded as SCALE
    let version_encoded = parity_scale_codec::Encode::encode(&version_num);
    let call_data = format!("0x{}", hex::encode(&version_encoded));

    let result: String = state
        .rpc_client
        .request(
            "state_call",
            rpc_params!["Metadata_metadata_at_version", &call_data, &block_hash],
        )
        .await
        .map_err(|e| {
            let err_str = e.to_string().to_lowercase();
            if err_str.contains("not found") || err_str.contains("does not exist") {
                GetMetadataError::MetadataVersionsNotAvailable
            } else {
                GetMetadataError::RpcFailed(e)
            }
        })?;

    // Decode the result - it's an Option<OpaqueMetadata>
    let hex_str = result.strip_prefix("0x").unwrap_or(&result);
    let bytes = hex::decode(hex_str).map_err(GetMetadataError::HexDecodeFailed)?;

    // The result is Option<OpaqueMetadata> where OpaqueMetadata is Vec<u8>
    // Option encoding: 0x00 = None, 0x01 + data = Some
    if bytes.is_empty() || bytes[0] == 0 {
        return Err(GetMetadataError::VersionNotAvailable(version_num));
    }

    // Skip the Option Some byte (0x01) and decode the inner Vec<u8>
    let opaque_metadata: Vec<u8> =
        Vec::<u8>::decode(&mut &bytes[1..]).map_err(GetMetadataError::ScaleDecodeFailed)?;

    if opaque_metadata.len() < 4 {
        return Err(GetMetadataError::MetadataTooShort);
    }

    // Magic number is the first 4 bytes as little-endian u32
    let magic_number = u32::from_le_bytes(
        opaque_metadata[0..4]
            .try_into()
            .map_err(|_| GetMetadataError::MetadataTooShort)?,
    );

    let metadata_prefixed = RuntimeMetadataPrefixed::decode(&mut &opaque_metadata[..])
        .map_err(GetMetadataError::ScaleDecodeFailed)?;

    let metadata = convert_metadata(&metadata_prefixed.1)?;

    Ok(Json(RuntimeMetadataResponse {
        magic_number: magic_number.to_string(),
        metadata,
    }))
}

pub fn convert_metadata(metadata: &RuntimeMetadata) -> Result<Value, GetMetadataError> {
    match metadata {
        RuntimeMetadata::V14(m) => Ok(json!({ "v14": convert_v14(m) })),
        RuntimeMetadata::V15(m) => Ok(json!({ "v15": convert_v15(m) })),
        _ => Err(GetMetadataError::UnsupportedVersion),
    }
}

// ============================================================================
// V14 Conversion
// ============================================================================

fn convert_v14(m: &v14_types::RuntimeMetadataV14) -> Value {
    json!({
        "lookup": convert_portable_registry(&m.types),
        "pallets": m.pallets.iter().map(|p| convert_v14_pallet(p, &m.types)).collect::<Vec<_>>(),
        "extrinsic": convert_v14_extrinsic(&m.extrinsic),
        "type": m.ty.id.to_string()
    })
}

fn convert_portable_registry(registry: &PortableRegistry) -> Value {
    json!({
        "types": registry.types.iter().map(|t| {
            json!({
                "id": t.id.to_string(),
                "type": convert_portable_type(&t.ty)
            })
        }).collect::<Vec<_>>()
    })
}

fn convert_portable_type(ty: &scale_info::Type<PortableForm>) -> Value {
    json!({
        "path": ty.path.segments.clone(),
        "params": ty.type_params.iter().map(|p| {
            json!({
                "name": p.name.clone(),
                "type": p.ty.map(|t| t.id.to_string())
            })
        }).collect::<Vec<_>>(),
        "def": convert_type_def(&ty.type_def),
        "docs": ty.docs.clone()
    })
}

fn convert_type_def(def: &scale_info::TypeDef<PortableForm>) -> Value {
    use scale_info::TypeDef;
    match def {
        TypeDef::Composite(c) => json!({
            "composite": {
                "fields": c.fields.iter().map(convert_field).collect::<Vec<_>>()
            }
        }),
        TypeDef::Variant(v) => json!({
            "variant": {
                "variants": v.variants.iter().map(|var| {
                    json!({
                        "name": var.name.clone(),
                        "fields": var.fields.iter().map(convert_field).collect::<Vec<_>>(),
                        "index": var.index.to_string(),
                        "docs": var.docs.clone()
                    })
                }).collect::<Vec<_>>()
            }
        }),
        TypeDef::Sequence(s) => json!({
            "sequence": {
                "type": s.type_param.id.to_string()
            }
        }),
        TypeDef::Array(a) => json!({
            "array": {
                "len": a.len.to_string(),
                "type": a.type_param.id.to_string()
            }
        }),
        TypeDef::Tuple(t) => json!({
            "tuple": t.fields.iter().map(|f| f.id.to_string()).collect::<Vec<_>>()
        }),
        TypeDef::Primitive(p) => json!({
            "primitive": format!("{:?}", p)
        }),
        TypeDef::Compact(c) => json!({
            "compact": {
                "type": c.type_param.id.to_string()
            }
        }),
        TypeDef::BitSequence(b) => json!({
            "bitSequence": {
                "bitStoreType": b.bit_store_type.id.to_string(),
                "bitOrderType": b.bit_order_type.id.to_string()
            }
        }),
    }
}

fn convert_field(f: &scale_info::Field<PortableForm>) -> Value {
    json!({
        "name": f.name.clone(),
        "type": f.ty.id.to_string(),
        "typeName": f.type_name.clone(),
        "docs": f.docs.clone()
    })
}

fn convert_v14_pallet(
    p: &v14_types::PalletMetadata<PortableForm>,
    registry: &PortableRegistry,
) -> Value {
    json!({
        "name": p.name.clone(),
        "storage": p.storage.as_ref().map(convert_v14_storage),
        "calls": p.calls.as_ref().map(|c| json!({ "type": c.ty.id.to_string() })),
        "events": p.event.as_ref().map(|e| json!({ "type": e.ty.id.to_string() })),
        "constants": p.constants.iter().map(|c| convert_v14_constant(c, registry)).collect::<Vec<_>>(),
        "errors": p.error.as_ref().map(|e| json!({ "type": e.ty.id.to_string() })),
        "index": p.index.to_string()
    })
}

fn convert_v14_storage(s: &v14_types::PalletStorageMetadata<PortableForm>) -> Value {
    json!({
        "prefix": s.prefix.clone(),
        "items": s.entries.iter().map(convert_v14_storage_entry).collect::<Vec<_>>()
    })
}

fn convert_v14_storage_entry(e: &v14_types::StorageEntryMetadata<PortableForm>) -> Value {
    json!({
        "name": e.name.clone(),
        "modifier": format!("{:?}", e.modifier),
        "type": convert_storage_entry_type(&e.ty),
        "fallback": format!("0x{}", hex::encode(&e.default)),
        "docs": e.docs.clone()
    })
}

fn convert_storage_entry_type(ty: &v14_types::StorageEntryType<PortableForm>) -> Value {
    match ty {
        v14_types::StorageEntryType::Plain(t) => json!({ "plain": t.id.to_string() }),
        v14_types::StorageEntryType::Map {
            hashers,
            key,
            value,
        } => json!({
            "map": {
                "hashers": hashers.iter().map(|h| format!("{:?}", h)).collect::<Vec<_>>(),
                "key": key.id.to_string(),
                "value": value.id.to_string()
            }
        }),
    }
}

fn convert_v14_constant(
    c: &v14_types::PalletConstantMetadata<PortableForm>,
    registry: &PortableRegistry,
) -> Value {
    // Decode the constant value based on its type
    let value = decode_constant_value(&c.value, c.ty.id, registry);

    json!({
        "name": c.name.clone(),
        "type": c.ty.id.to_string(),
        "value": value,
        "docs": c.docs.clone()
    })
}

/// Decode a constant value based on its type.
/// For primitive integer types (u8, u16, u32, u64, u128), decode to decimal string.
/// For Option<primitive> types, decode the inner value if present.
/// For specific newtype wrappers (like ParaId), decode the inner value.
/// For other types, return hex-encoded bytes.
fn decode_constant_value(bytes: &[u8], type_id: u32, registry: &PortableRegistry) -> String {
    use scale_info::TypeDef;

    // Look up the type definition
    if let Some(ty) = registry.resolve(type_id) {
        match &ty.type_def {
            TypeDef::Primitive(p) => {
                use scale_info::TypeDefPrimitive;
                match p {
                    TypeDefPrimitive::U8 if !bytes.is_empty() => {
                        return bytes[0].to_string();
                    }
                    TypeDefPrimitive::U16 if bytes.len() >= 2 => {
                        let val = u16::from_le_bytes(bytes[0..2].try_into().unwrap());
                        return val.to_string();
                    }
                    TypeDefPrimitive::U32 if bytes.len() >= 4 => {
                        let val = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
                        return val.to_string();
                    }
                    TypeDefPrimitive::U64 if bytes.len() >= 8 => {
                        let val = u64::from_le_bytes(bytes[0..8].try_into().unwrap());
                        return val.to_string();
                    }
                    TypeDefPrimitive::U128 if bytes.len() >= 16 => {
                        let val = u128::from_le_bytes(bytes[0..16].try_into().unwrap());
                        return val.to_string();
                    }
                    _ => {}
                }
            }
            TypeDef::Composite(c) => {
                // Handle specific newtype patterns that should be decoded to decimal
                let type_name = ty.path.segments.last().map(|s| s.as_str()).unwrap_or("");

                // Decode single-field composite types:
                // 1. ID-like types (ParaId, CoreIndex, etc.)
                // 2. Fixed-point types (FixedU128, etc.) - Sidecar decodes these to decimal strings
                let is_id_type = ty.path.segments.iter().any(|s| {
                    let seg = s.as_str();
                    seg == "Id" || seg.ends_with("Id") || seg.ends_with("Index")
                });

                let is_fixed_point = type_name.starts_with("Fixed");

                let should_decode = is_id_type || is_fixed_point;

                if should_decode && c.fields.len() == 1 {
                    let field = &c.fields[0];
                    // Recursively decode - this handles nested newtypes and primitives
                    let decoded = decode_constant_value(bytes, field.ty.id, registry);
                    // Only use the decoded value if it's not hex (meaning it was successfully decoded)
                    if !decoded.starts_with("0x") {
                        return decoded;
                    }
                }
            }
            TypeDef::Variant(v) => {
                // Check if this is an Option type (has None and Some variants)
                let is_option = ty.path.segments.last().map(|s| s.as_str()) == Some("Option");
                if is_option && !bytes.is_empty() && bytes[0] == 1 {
                    // This is Some variant (index 1)
                    // Find the Some variant and get its inner type
                    if let Some(field) = v
                        .variants
                        .iter()
                        .find(|var| var.name == "Some")
                        .and_then(|some_variant| some_variant.fields.first())
                    {
                        // Recursively decode the inner value (skip the variant index byte)
                        let decoded = decode_constant_value(&bytes[1..], field.ty.id, registry);
                        // Only use the decoded value if it's not hex
                        if !decoded.starts_with("0x") {
                            return decoded;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Default: hex-encoded bytes
    format!("0x{}", hex::encode(bytes))
}

fn convert_v14_extrinsic(e: &v14_types::ExtrinsicMetadata<PortableForm>) -> Value {
    json!({
        "type": e.ty.id.to_string(),
        "version": e.version.to_string(),
        "signedExtensions": e.signed_extensions.iter().map(|ext| {
            json!({
                "identifier": ext.identifier.clone(),
                "type": ext.ty.id.to_string(),
                "additionalSigned": ext.additional_signed.id.to_string()
            })
        }).collect::<Vec<_>>()
    })
}

// ============================================================================
// V15 Conversion
// ============================================================================

fn convert_v15(m: &v15_types::RuntimeMetadataV15) -> Value {
    json!({
        "lookup": convert_portable_registry(&m.types),
        "pallets": m.pallets.iter().map(|p| convert_v15_pallet(p, &m.types)).collect::<Vec<_>>(),
        "extrinsic": convert_v15_extrinsic(&m.extrinsic),
        "type": m.ty.id.to_string(),
        "apis": m.apis.iter().map(convert_runtime_api).collect::<Vec<_>>(),
        "outerEnums": convert_outer_enums(&m.outer_enums),
        "custom": convert_custom_metadata(&m.custom)
    })
}

fn convert_v15_pallet(
    p: &v15_types::PalletMetadata<PortableForm>,
    registry: &PortableRegistry,
) -> Value {
    json!({
        "name": p.name.clone(),
        "storage": p.storage.as_ref().map(convert_v15_storage),
        "calls": p.calls.as_ref().map(|c| json!({ "type": c.ty.id.to_string() })),
        "events": p.event.as_ref().map(|e| json!({ "type": e.ty.id.to_string() })),
        "constants": p.constants.iter().map(|c| convert_v15_constant(c, registry)).collect::<Vec<_>>(),
        "errors": p.error.as_ref().map(|e| json!({ "type": e.ty.id.to_string() })),
        "index": p.index.to_string(),
        "docs": p.docs.clone()
    })
}

fn convert_v15_storage(s: &v15_types::PalletStorageMetadata<PortableForm>) -> Value {
    json!({
        "prefix": s.prefix.clone(),
        "items": s.entries.iter().map(convert_v15_storage_entry).collect::<Vec<_>>()
    })
}

fn convert_v15_storage_entry(e: &v15_types::StorageEntryMetadata<PortableForm>) -> Value {
    json!({
        "name": e.name.clone(),
        "modifier": format!("{:?}", e.modifier),
        "type": convert_v15_storage_entry_type(&e.ty),
        "fallback": format!("0x{}", hex::encode(&e.default)),
        "docs": e.docs.clone()
    })
}

fn convert_v15_storage_entry_type(ty: &v15_types::StorageEntryType<PortableForm>) -> Value {
    match ty {
        v15_types::StorageEntryType::Plain(t) => json!({ "plain": t.id.to_string() }),
        v15_types::StorageEntryType::Map {
            hashers,
            key,
            value,
        } => json!({
            "map": {
                "hashers": hashers.iter().map(|h| format!("{:?}", h)).collect::<Vec<_>>(),
                "key": key.id.to_string(),
                "value": value.id.to_string()
            }
        }),
    }
}

fn convert_v15_constant(
    c: &v15_types::PalletConstantMetadata<PortableForm>,
    registry: &PortableRegistry,
) -> Value {
    let value = decode_constant_value(&c.value, c.ty.id, registry);

    json!({
        "name": c.name.clone(),
        "type": c.ty.id.to_string(),
        "value": value,
        "docs": c.docs.clone()
    })
}

fn convert_v15_extrinsic(e: &v15_types::ExtrinsicMetadata<PortableForm>) -> Value {
    json!({
        "version": e.version.to_string(),
        "address": e.address_ty.id.to_string(),
        "call": e.call_ty.id.to_string(),
        "signature": e.signature_ty.id.to_string(),
        "extra": e.extra_ty.id.to_string(),
        "signedExtensions": e.signed_extensions.iter().map(|ext| {
            json!({
                "identifier": ext.identifier.clone(),
                "type": ext.ty.id.to_string(),
                "additionalSigned": ext.additional_signed.id.to_string()
            })
        }).collect::<Vec<_>>()
    })
}

fn convert_runtime_api(api: &v15_types::RuntimeApiMetadata<PortableForm>) -> Value {
    json!({
        "name": api.name.clone(),
        "methods": api.methods.iter().map(|m| {
            json!({
                "name": m.name.clone(),
                "inputs": m.inputs.iter().map(|i| {
                    json!({
                        "name": i.name.clone(),
                        "type": i.ty.id.to_string()
                    })
                }).collect::<Vec<_>>(),
                "output": m.output.id.to_string(),
                "docs": m.docs.clone()
            })
        }).collect::<Vec<_>>(),
        "docs": api.docs.clone()
    })
}

fn convert_outer_enums(e: &v15_types::OuterEnums<PortableForm>) -> Value {
    json!({
        "callType": e.call_enum_ty.id.to_string(),
        "eventType": e.event_enum_ty.id.to_string(),
        "errorType": e.error_enum_ty.id.to_string()
    })
}

fn convert_custom_metadata(c: &v15_types::CustomMetadata<PortableForm>) -> Value {
    json!({
        "map": c.map.iter().map(|(k, v)| {
            (k.clone(), json!({
                "type": v.ty.id.to_string(),
                "value": format!("0x{}", hex::encode(&v.value))
            }))
        }).collect::<serde_json::Map<String, Value>>()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_number_calculation() {
        // "meta" in ASCII bytes = [0x6d, 0x65, 0x74, 0x61]
        // As little-endian u32 = 1635018093
        let bytes: [u8; 4] = [0x6d, 0x65, 0x74, 0x61];
        let magic = u32::from_le_bytes(bytes);
        assert_eq!(magic, 1635018093);
    }

    #[test]
    fn test_response_serialization() {
        let response = RuntimeMetadataResponse {
            magic_number: "1635018093".to_string(),
            metadata: json!({"v14": {}}),
        };
        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["magicNumber"], "1635018093");
    }
}
