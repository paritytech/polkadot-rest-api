// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Extrinsic extraction and processing.
//!
//! This module handles extracting extrinsics from blocks, including:
//! - Decoding call arguments with type-aware transformations
//! - Extracting signatures, nonces, tips, and era information
//! - Converting account addresses to SS58 format

use crate::state::{AppState, SubstrateLegacyRpc};
use crate::utils::{
    self, ChargeAssetTxPayment, ChargeTransactionPayment, CheckNonce, DecodedExtrinsic, EraInfo,
    fetch_block_body,
};
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

/// Extract extrinsics from a block with an explicit ss58_prefix
///
/// This version allows specifying the ss58_prefix explicitly, useful for
/// processing blocks from different chains (e.g., relay chain blocks).
///
/// `legacy_rpc` must talk to the same chain as `client_at_block`; it is used to
/// fetch the raw block body.
pub async fn extract_extrinsics_with_prefix(
    ss58_prefix: u16,
    legacy_rpc: &SubstrateLegacyRpc,
    client_at_block: &BlockClient,
    block_number: u64,
) -> Result<Vec<ExtrinsicInfo>, GetBlockError> {
    extract_extrinsics_impl(ss58_prefix, legacy_rpc, client_at_block, block_number).await
}

/// Extract extrinsics from a block
pub async fn extract_extrinsics(
    state: &AppState,
    client_at_block: &BlockClient,
    block_number: u64,
) -> Result<Vec<ExtrinsicInfo>, GetBlockError> {
    extract_extrinsics_impl(
        state.chain_info.ss58_prefix,
        &state.legacy_rpc,
        client_at_block,
        block_number,
    )
    .await
}

/// Internal implementation for extracting extrinsics
async fn extract_extrinsics_impl(
    ss58_prefix: u16,
    legacy_rpc: &SubstrateLegacyRpc,
    client_at_block: &BlockClient,
    block_number: u64,
) -> Result<Vec<ExtrinsicInfo>, GetBlockError> {
    // Get the type resolver from metadata for type-aware enum serialization
    let metadata = client_at_block.metadata();
    let resolver = metadata.types();

    // We decode the block body ourselves via `DecodedExtrinsic` rather than using
    // `client_at_block.extrinsics()`, because subxt decodes V4 extrinsics against
    // the newest transaction extension version the runtime exposes instead of
    // version 0. See `crate::utils::extrinsic_decode` for the full story.
    let body = match fetch_block_body(legacy_rpc, client_at_block.block_hash()).await {
        Ok(body) => body,
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

    let mut result = Vec::with_capacity(body.len());

    for (extrinsic_index, extrinsic_bytes) in body.into_iter().enumerate() {
        let info = match utils::decode_extrinsic_info(&extrinsic_bytes, &metadata) {
            Ok(info) => info,
            Err(e) => {
                // Skipping leaves a hole in the block response, so make it loud and
                // log enough of the bytes to reproduce the failure offline. Inherents
                // can be tens of kilobytes, so cap what we print.
                const MAX_LOGGED_BYTES: usize = 1024;
                let logged = &extrinsic_bytes[..extrinsic_bytes.len().min(MAX_LOGGED_BYTES)];
                tracing::error!(
                    block_number,
                    extrinsic_index,
                    len = extrinsic_bytes.len(),
                    raw = %format!(
                        "0x{}{}",
                        hex::encode(logged),
                        if logged.len() < extrinsic_bytes.len() { "…" } else { "" }
                    ),
                    error = %e,
                    "Failed to decode extrinsic; it will be missing from the block response"
                );
                continue;
            }
        };

        let extrinsic =
            DecodedExtrinsic::new(extrinsic_index, extrinsic_bytes, info, metadata.clone());

        // Extract pallet and method name from the call, converting to lowerCamelCase
        let pallet_name = extrinsic.pallet_name().to_lower_camel_case();
        let method_name = extrinsic.call_name().to_lower_camel_case();

        // Extract call arguments with field-name-based AccountId32 detection
        let mut args_map = serde_json::Map::new();

        for field in extrinsic.iter_call_data_fields() {
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
            match field.visit(JsonVisitor::new(ss58_prefix, resolver)) {
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

            // Try to extract era from the transaction extensions payload. The payload
            // range is computed by frame-decode, so it is correct regardless of the
            // compact length prefix that `extrinsic.bytes()` carries (the raw block
            // body entry is length-prefixed), and it already excludes the extension
            // version byte for v5 General extrinsics. Era is the first explicit
            // field of the extensions payload.
            //
            // Note: do NOT pass `extrinsic.bytes()` to
            // `extract_era_from_extrinsic_bytes` here — those bytes include the
            // compact length prefix, which that parser would misread as the
            // version byte (see its docs).
            let era_info = extrinsic.transaction_extensions_bytes().and_then(|ext| {
                let mut offset = 0;
                utils::decode_era_from_bytes(ext, &mut offset)
            });

            let signer_hex = format!("0x{}", hex::encode(addr_bytes));
            let signer_ss58 = utils::decode_address_to_ss58(&signer_hex, ss58_prefix)
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
                        // Decode nonce directly using explicit type
                        let bytes = ext.bytes();
                        if let Ok(nonce) =
                            <CheckNonce as parity_scale_codec::Decode>::decode(&mut &bytes[..])
                        {
                            nonce_value = Some(nonce.0.to_string());
                        }
                    }
                    "ChargeTransactionPayment" => {
                        // Decode tip directly using explicit type
                        let bytes = ext.bytes();
                        if let Ok(payment) =
                            <ChargeTransactionPayment as parity_scale_codec::Decode>::decode(
                                &mut &bytes[..],
                            )
                        {
                            tip_value = Some(payment.0.to_string());
                        } else {
                            tip_value = Some("0".to_string());
                        }
                    }
                    "ChargeAssetTxPayment" => {
                        // Decode tip from ChargeAssetTxPayment struct
                        let bytes = ext.bytes();
                        if let Ok(payment) =
                            <ChargeAssetTxPayment as parity_scale_codec::Decode>::decode(
                                &mut &bytes[..],
                            )
                        {
                            tip_value = Some(payment.tip.to_string());
                        } else {
                            tip_value = Some("0".to_string());
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::{
        TEST_BLOCK_NUMBER, TEST_GENESIS_HASH, mock_rpc_client_builder, mock_rpc_client_builder_v16,
    };
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use subxt_rpcs::client::mock_rpc_client::{Json as MockJson, MockRpcClientBuilder};
    use subxt_rpcs::client::{MockRpcClient, RpcClient};
    use subxt_rpcs::methods::legacy::LegacyRpcMethods;

    /// Real Asset Hub extrinsic (block 17742975, #2), exactly as it appears
    /// in a `chain_getBlock` response: the block-body entry is
    /// length-prefixed (compact prefix `0xbd 0x01`). Era: Mortal(32, 11).
    const REAL_PREFIXED_EXTRINSIC: &str = "0xbd01840072284f32719a49037a79da881b91b44bf642395ecba92b241619e21fb1c8a57a01b250abd5b7715a993a111d0db2f6742a8b108fd5a700b5c9e443f9fb14f79938d668ba315e48121b2bd2584b104014e6ede84fe35b77adcc9ff8030de5daef8ab4003e7b0100000000000000";

    /// Polkadot Asset Hub block 20487777, extrinsic #1: a V5 `Bare`
    /// `Timestamp.set` inherent, as it appears in `chain_getBlock`.
    const AH_20487777_V5_TIMESTAMP_SET: &str = "0x280503000b80e44f8ba001";

    /// Polkadot Asset Hub block 20487777, extrinsic #2: a V4 **signed**
    /// `Balances.transfer_allow_death`.
    ///
    /// This is one of the two extrinsics reported missing in #405. Its era byte
    /// (`0x79`) is what subxt misread as a variant index of a version-1-only
    /// transaction extension, producing `VariantNotFound(121)`.
    const AH_20487777_V4_TRANSFER_ALLOW_DEATH: &str = "0x59028400bf2f813dcbe2d0f9cb7b9dc61e46b2bcae7b6783adf184516229c891bc5359f500e8c8525ab31ec292d3762a583a93f2ca5f7b451185dd2c5a491e80e1fabf05235f83656132074cb9e6be03d7b25a40bd70af0861edfe7961ccb7d8894d0729007925152b025a620200000a00007a3584265306b5f6a6bd158f57e490c0f32d1135301b2fc89d7d8f76362af69007008824bf68";

    /// Polkadot Asset Hub block 20487777, extrinsic #3: a V4 **signed**
    /// `Balances.transfer_keep_alive`. The second extrinsic reported missing in
    /// #405; its era byte (`0x58`) produced `VariantNotFound(88)`.
    const AH_20487777_V4_TRANSFER_KEEP_ALIVE: &str = "0x55028400dc0c5e6f6c8265265f0bb9bbfa5c46a2471c05152066ed925e450361ad229913003132c42127d205558668f484efeafe4edcc8b49197e1f3f408774c411abc541d167c4f365bc62462bc2a46ca5db6284bf55bb48713171f15903e55cf7c415d01580529130000000a03001d46ccb04c50f93bde3457fd8e1dcee7da4ae2b8719f6d1df89f25afd75557c90f00506d96f51401";

    /// Answer `chain_getBlock` with a body containing exactly `extrinsics`.
    fn with_block_body(
        builder: MockRpcClientBuilder,
        extrinsics: Vec<&'static str>,
    ) -> MockRpcClient {
        builder
            .method_handler("chain_getBlock", move |_params| {
                let extrinsics = extrinsics.clone();
                async move {
                    MockJson(json!({
                        "block": {
                            "header": {
                                "number": format!("0x{:x}", TEST_BLOCK_NUMBER),
                                "parentHash": TEST_GENESIS_HASH,
                                "stateRoot": TEST_GENESIS_HASH,
                                "extrinsicsRoot": TEST_GENESIS_HASH,
                                "digest": { "logs": [] }
                            },
                            "extrinsics": extrinsics
                        },
                        "justifications": null
                    }))
                }
            })
            .build()
    }

    /// Extract the extrinsics of the mocked block over the given mock client.
    async fn extract_from_mock(mock: MockRpcClient) -> Vec<ExtrinsicInfo> {
        let rpc_client = RpcClient::new(mock);
        let legacy_rpc = LegacyRpcMethods::new(rpc_client.clone());
        let client = subxt::OnlineClient::<subxt::SubstrateConfig>::from_rpc_client(rpc_client)
            .await
            .expect("Failed to create OnlineClient");
        let at_block = client
            .at_current_block()
            .await
            .expect("Failed at_current_block");

        extract_extrinsics_with_prefix(0, &legacy_rpc, &at_block, TEST_BLOCK_NUMBER)
            .await
            .expect("extraction should succeed")
    }

    /// Regression test for #405: on a runtime exposing transaction extension
    /// versions `[0, 1]`, V4 signed extrinsics must still be decoded (against
    /// version 0) and returned rather than dropped from the block response.
    ///
    /// Before the fix, subxt decoded these two V4 extrinsics against version 1 —
    /// which prepends `UnitTransactionExtension` and `VerifyMultiSignature` — read
    /// the era bytes as enum variant indexes, failed with `VariantNotFound(121)`
    /// and `VariantNotFound(88)`, and this function skipped both. The block came
    /// back with 1 of 3 extrinsics.
    #[tokio::test]
    async fn test_v4_signed_extrinsics_survive_multiple_extension_versions() {
        let mock = with_block_body(
            mock_rpc_client_builder_v16(),
            vec![
                AH_20487777_V5_TIMESTAMP_SET,
                AH_20487777_V4_TRANSFER_ALLOW_DEATH,
                AH_20487777_V4_TRANSFER_KEEP_ALIVE,
            ],
        );

        let extrinsics = extract_from_mock(mock).await;

        let methods: Vec<(&str, &str)> = extrinsics
            .iter()
            .map(|e| (e.method.pallet.as_str(), e.method.method.as_str()))
            .collect();
        assert_eq!(
            methods,
            vec![
                ("timestamp", "set"),
                ("balances", "transferAllowDeath"),
                ("balances", "transferKeepAlive"),
            ],
            "V4 signed extrinsics were dropped from the block response"
        );

        // The inherent is unsigned; the two V4 transfers are signed and must carry
        // their signer, nonce, tip and era.
        assert!(extrinsics[0].signature.is_none());
        for extrinsic in &extrinsics[1..] {
            let signature = extrinsic
                .signature
                .as_ref()
                .expect("V4 signed extrinsic should have a signature");
            assert!(
                signature.signer.id.starts_with('1'),
                "expected an SS58 signer"
            );
            assert!(extrinsic.nonce.is_some(), "nonce should be decoded");
            assert!(extrinsic.tip.is_some(), "tip should be decoded");
            assert!(
                extrinsic.era.mortal_era.is_some(),
                "era should be decoded as mortal"
            );
        }
    }

    /// `MakeWriter` that appends everything to a shared buffer so a test can
    /// assert on emitted tracing output.
    #[derive(Clone, Default)]
    struct CaptureWriter(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for CaptureWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CaptureWriter {
        type Writer = CaptureWriter;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    /// Call-site regression test for the length-prefix era bug: process a
    /// block whose body entry is a real length-prefixed extrinsic and assert
    /// both the era output and that no era-decode warning is emitted.
    ///
    /// The warning assertion is the discriminating part: before #370 this
    /// call site passed `extrinsic.bytes()` (which include the compact
    /// length prefix) to `extract_era_from_extrinsic_bytes` for every signed
    /// extrinsic, which misread the prefix byte as the version byte, walked
    /// garbage, and logged `Failed to decode Era from bytes at offset 0` —
    /// the WARN flood reported in #369. Reverting the call site to the
    /// prefixed-bytes walk turns this test red.
    #[tokio::test]
    async fn test_extract_extrinsics_prefixed_body_entry_era_without_warning() {
        let mock = with_block_body(mock_rpc_client_builder(), vec![REAL_PREFIXED_EXTRINSIC]);

        // Capture WARN-level tracing output while extracting. This relies on
        // the current-thread tokio runtime of #[tokio::test]: set_default is
        // thread-local, so it sees everything the extraction emits.
        let captured = CaptureWriter::default();
        let subscriber = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::WARN)
            .with_writer(captured.clone())
            .finish();
        let _guard = tracing::subscriber::set_default(subscriber);

        let extrinsics = extract_from_mock(mock).await;

        assert_eq!(extrinsics.len(), 1);
        assert_eq!(
            extrinsics[0].era.mortal_era,
            Some(vec!["32".to_string(), "11".to_string()])
        );
        assert_eq!(extrinsics[0].era.immortal_era, None);

        let logs = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
        assert!(
            !logs.contains("Failed to decode Era"),
            "era decode warning emitted while processing a length-prefixed \
             block-body entry (the #369 WARN flood):\n{logs}"
        );
    }
}
