//! Extrinsic extraction and processing.
//!
//! This module handles extracting extrinsics from blocks, including:
//! - Decoding call arguments with type-aware transformations
//! - Extracting signatures, nonces, tips, and era information
//! - Converting account addresses to SS58 format

use crate::state::AppState;
use crate::utils::{self, EraInfo};
use heck::ToLowerCamelCase;
use serde_json::{Value, json};
use sp_core::crypto::{AccountId32, Ss58Codec};
use sp_runtime::traits::BlakeTwo256;
use sp_runtime::traits::Hash as HashT;

use super::super::common::BlockClient;
use super::super::decode::{GetTypeName, JsonVisitor};
use super::super::types::{
    ExtrinsicInfo, GetBlockError, MethodInfo, MultiAddress, SignatureInfo, SignerId,
};
use super::super::utils::extract_numeric_string;

/// Extract extrinsics from a block using subxt-historic
pub async fn extract_extrinsics(
    state: &AppState,
    client_at_block: &BlockClient<'_>,
    block_number: u64,
) -> Result<Vec<ExtrinsicInfo>, GetBlockError> {
    // Get the resolver for type-aware enum serialization
    let resolver = client_at_block.resolver();

    let extrinsics = match client_at_block.extrinsics().fetch().await {
        Ok(exts) => exts,
        Err(e) => {
            // This could indicate RPC issues or network problems
            tracing::warn!(
                "Failed to fetch extrinsics for block {}: {:?}. Returning empty extrinsics.",
                block_number,
                e
            );
            return Ok(Vec::new());
        }
    };

    let mut result = Vec::new();

    for extrinsic in extrinsics.iter() {
        // Extract pallet and method name from the call, converting to lowerCamelCase
        let pallet_name = extrinsic.call().pallet_name().to_lower_camel_case();
        let method_name = extrinsic.call().name().to_lower_camel_case();

        // Extract call arguments with field-name-based AccountId32 detection
        let fields = extrinsic.call().fields();
        let mut args_map = serde_json::Map::new();

        for field in fields.iter() {
            let field_name = field.name();
            // Keep field names as-is (snake_case from SCALE metadata)
            // Only nested object keys are transformed to camelCase via transform_json_unified
            let field_key = field_name.to_string();

            // Use the visitor pattern to get type information
            // This definitively detects AccountId32 fields by their actual type!
            let type_name = field.visit(GetTypeName::new()).ok().flatten();

            // Log the type name for demonstration
            if let Some(tn) = type_name {
                tracing::debug!(
                    "Field '{}' in {}.{} has type: {}",
                    field_name,
                    pallet_name,
                    method_name,
                    tn
                );
            }

            // Try to decode as AccountId32-related types based on the detected type name
            let is_account_type = type_name == Some("AccountId32")
                || type_name == Some("MultiAddress")
                || type_name == Some("AccountId");

            if is_account_type {
                let mut decoded_account = false;
                let ss58_prefix = state.chain_info.ss58_prefix;
                let bytes_to_ss58 = |bytes: &[u8; 32]| {
                    let account_id = AccountId32::from(*bytes);
                    account_id.to_ss58check_with_version(ss58_prefix.into())
                };

                if let Ok(account_bytes) = field.decode_as::<[u8; 32]>() {
                    let ss58 = bytes_to_ss58(&account_bytes);
                    args_map.insert(field_key.clone(), json!(ss58));
                    decoded_account = true;
                } else if let Ok(accounts) = field.decode_as::<Vec<[u8; 32]>>() {
                    let ss58_addresses: Vec<String> = accounts.iter().map(&bytes_to_ss58).collect();
                    args_map.insert(field_key.clone(), json!(ss58_addresses));
                    decoded_account = true;
                } else if let Ok(multi_addr) = field.decode_as::<MultiAddress>() {
                    let value = match multi_addr {
                        MultiAddress::Id(bytes) => {
                            json!({ "id": bytes_to_ss58(&bytes) })
                        }
                        MultiAddress::Address32(bytes) => {
                            json!({ "address32": bytes_to_ss58(&bytes) })
                        }
                        MultiAddress::Index(index) => json!({ "index": index }),
                        MultiAddress::Raw(bytes) => {
                            json!({ "raw": format!("0x{}", hex::encode(bytes)) })
                        }
                        MultiAddress::Address20(bytes) => {
                            json!({ "address20": format!("0x{}", hex::encode(bytes)) })
                        }
                    };
                    args_map.insert(field_key.clone(), value);
                    decoded_account = true;
                }

                if decoded_account {
                    continue;
                }
                // If we failed to decode as account types, fall through to Value<()> decoding
            }

            // For non-account fields (or account fields that failed to decode):
            // Use the type-aware JsonVisitor which correctly handles:
            // - SS58 encoding only for AccountId32/MultiAddress/AccountId types
            // - Preserving arrays for Vec<T> sequences
            // - Converting byte arrays to hex
            // - Basic enums as strings, non-basic enums as objects
            match field.visit(JsonVisitor::new(state.chain_info.ss58_prefix, &resolver)) {
                Ok(json_value) => {
                    args_map.insert(field_key, json_value);
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to decode field '{}' in {}.{}: {}",
                        field_name,
                        pallet_name,
                        method_name,
                        e
                    );
                }
            }
        }

        // Extract signature and signer (if signed)
        let (signature_info, era_from_bytes) = if extrinsic.is_signed() {
            let sig_bytes = extrinsic
                .signature_bytes()
                .ok_or(GetBlockError::MissingSignatureBytes)?;
            let addr_bytes = extrinsic
                .address_bytes()
                .ok_or(GetBlockError::MissingAddressBytes)?;

            // Try to extract era from raw extrinsic bytes
            // Era comes right after address and signature in the SignedExtra/TransactionExtension
            let era_info = utils::extract_era_from_extrinsic_bytes(extrinsic.bytes());

            let signer_hex = format!("0x{}", hex::encode(addr_bytes));
            let signer_ss58 =
                utils::decode_address_to_ss58(&signer_hex, state.chain_info.ss58_prefix)
                    .unwrap_or_else(|| signer_hex.clone());

            // Strip the signature type prefix byte (0x00=Ed25519, 0x01=Sr25519, 0x02=Ecdsa)
            let signature_without_type_prefix = if sig_bytes.len() > 1 {
                &sig_bytes[1..]
            } else {
                sig_bytes
            };

            (
                Some(SignatureInfo {
                    signature: format!("0x{}", hex::encode(signature_without_type_prefix)),
                    signer: SignerId { id: signer_ss58 },
                }),
                era_info,
            )
        } else {
            (None, None)
        };

        // Extract nonce, tip, and era from transaction extensions (if present)
        let (nonce, tip, era_info) = if let Some(extensions) = extrinsic.transaction_extensions() {
            let mut nonce_value = None;
            let mut tip_value = None;
            let mut era_value = None;

            tracing::trace!(
                "Extrinsic {} has {} extensions",
                extrinsic.index(),
                extensions.iter().count()
            );

            for ext in extensions.iter() {
                let ext_name = ext.name();
                tracing::trace!("Extension name: {}", ext_name);

                match ext_name {
                    "CheckNonce" => {
                        // Decode as a u64/u32 compact value, then serialize to JSON
                        if let Ok(n) = ext.decode_as::<scale_value::Value>()
                            && let Ok(json_val) = serde_json::to_value(&n)
                        {
                            // The value might be nested in an object, so we need to extract it
                            // If extraction fails, nonce_value remains None (serialized as null)
                            nonce_value = extract_numeric_string(&json_val);
                        }
                    }
                    "ChargeTransactionPayment" | "ChargeAssetTxPayment" => {
                        // The tip is typically a Compact<u128>
                        if let Ok(t) = ext.decode_as::<scale_value::Value>()
                            && let Ok(json_val) = serde_json::to_value(&t)
                        {
                            // If extraction fails, tip_value remains None (serialized as null)
                            tip_value = extract_numeric_string(&json_val);
                        }
                    }
                    "CheckMortality" | "CheckEra" => {
                        // Era information - decode directly from raw bytes
                        // The JSON representation is complex (e.g., "Mortal230") and harder to parse
                        let era_bytes = ext.bytes();
                        tracing::debug!(
                            "Found CheckMortality extension, raw bytes: {}",
                            hex::encode(era_bytes)
                        );

                        let mut offset = 0;
                        if let Some(decoded_era) =
                            utils::decode_era_from_bytes(era_bytes, &mut offset)
                        {
                            tracing::debug!("Decoded era: {:?}", decoded_era);

                            // Create a JSON representation that parse_era_info can understand
                            if let Some(ref mortal) = decoded_era.mortal_era {
                                // Format: {"name": "Mortal", "values": [[period], [phase]]}
                                let mut map = serde_json::Map::new();
                                map.insert("name".to_string(), Value::String("Mortal".to_string()));

                                let values = vec![
                                    Value::Array(vec![Value::Number(
                                        mortal[0].parse::<u64>().unwrap().into(),
                                    )]),
                                    Value::Array(vec![Value::Number(
                                        mortal[1].parse::<u64>().unwrap().into(),
                                    )]),
                                ];
                                map.insert("values".to_string(), Value::Array(values));

                                era_value = Some(Value::Object(map));
                            } else if decoded_era.immortal_era.is_some() {
                                let mut map = serde_json::Map::new();
                                map.insert(
                                    "name".to_string(),
                                    Value::String("Immortal".to_string()),
                                );
                                era_value = Some(Value::Object(map));
                            }
                        }
                    }
                    _ => {
                        // Silently skip other extensions
                    }
                }
            }

            let era = if let Some(era_json) = era_value {
                // Try to parse era information from extension
                utils::parse_era_info(&era_json)
            } else if let Some(era_parsed) = era_from_bytes {
                // Use era extracted from raw bytes
                era_parsed
            } else {
                // Default to immortal era for signed transactions without explicit era
                EraInfo {
                    immortal_era: Some("0x00".to_string()),
                    mortal_era: None,
                }
            };

            (nonce_value, tip_value, era)
        } else {
            // Unsigned extrinsics are immortal
            (
                None,
                None,
                EraInfo {
                    immortal_era: Some("0x00".to_string()),
                    mortal_era: None,
                },
            )
        };

        let extrinsic_bytes = extrinsic.bytes();
        let hash_bytes = BlakeTwo256::hash(extrinsic_bytes);
        let hash = format!("0x{}", hex::encode(hash_bytes.as_ref()));
        let raw_hex = format!("0x{}", hex::encode(extrinsic_bytes));

        // Initialize pays_fee based on whether the extrinsic is signed:
        // - Unsigned extrinsics (inherents) never pay fees → Some(false)
        // - Signed extrinsics: determined from DispatchInfo in events → None (will be updated later)
        let is_signed = signature_info.is_some();
        let pays_fee = if is_signed { None } else { Some(false) };

        result.push(ExtrinsicInfo {
            method: MethodInfo {
                pallet: pallet_name,
                method: method_name,
            },
            signature: signature_info,
            nonce,
            args: args_map,
            tip,
            hash,
            info: serde_json::Map::new(),
            era: era_info,
            events: Vec::new(),
            success: false,
            pays_fee,
            docs: None, // Will be populated if extrinsicDocs=true
            raw_hex,
        });
    }

    Ok(result)
}
