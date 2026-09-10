// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Extrinsic decoding that is correct for V4 extrinsics on runtimes exposing
//! more than one transaction extension version.
//!
//! # Why this module exists
//!
//! A V4 extrinsic carries no transaction extension version byte: it is *defined*
//! to use version 0 of the runtime's transaction extensions. Only V5 `General`
//! extrinsics carry an explicit version byte.
//!
//! `frame_decode` models this by passing `None` as the extension version when it
//! decodes a V4 extrinsic, and asks the [`ExtrinsicTypeInfo`] implementation what
//! to do with it. `subxt_metadata::Metadata`'s implementation answers with
//! [`transaction_extension_version_to_use_for_decoding()`], which returns the
//! *highest* version in the metadata (see `paritytech/subxt#1998`).
//!
//! That was harmless while chains only ever exposed version 0. Polkadot Asset Hub
//! spec 2005000 exposes versions `[0, 1]`, where version 1 prepends extensions such
//! as `UnitTransactionExtension` and `VerifyMultiSignature`. Decoding a V4
//! extrinsic against version 1 reads the first bytes of the era as enum variant
//! indexes, and decoding fails with `VariantNotFound(..)`. subxt then yields an
//! error for that extrinsic and the block response loses it entirely.
//!
//! [`V4CompatMetadata`] wraps the metadata and answers `None` with version 0, which
//! is what the extrinsic format requires. V5 extrinsics are unaffected: they pass
//! their own version through and it is used verbatim.
//!
//! [`transaction_extension_version_to_use_for_decoding()`]:
//!     subxt_metadata::ExtrinsicMetadata::transaction_extension_version_to_use_for_decoding

use frame_decode::extrinsics::{
    ExtrinsicCallInfo, ExtrinsicDecodeError, ExtrinsicExtensionInfo, ExtrinsicExtensions,
    ExtrinsicInfoError, ExtrinsicOwned, ExtrinsicSignatureInfo, ExtrinsicTypeInfo,
    decode_extrinsic,
};
use scale_info::PortableRegistry;
use subxt::Metadata;
use subxt_metadata::ArcMetadata;

/// Wraps [`Metadata`] so that extrinsics carrying no transaction extension version
/// (that is, V4 extrinsics) are decoded against extension version 0 rather than
/// against the highest version the metadata happens to expose.
///
/// Every other part of [`ExtrinsicTypeInfo`] is delegated unchanged.
pub struct V4CompatMetadata<'a>(pub &'a Metadata);

impl ExtrinsicTypeInfo for V4CompatMetadata<'_> {
    type TypeId = u32;

    fn extrinsic_call_info_by_index(
        &self,
        pallet_index: u8,
        call_index: u8,
    ) -> Result<ExtrinsicCallInfo<'_, Self::TypeId>, ExtrinsicInfoError<'_>> {
        self.0
            .extrinsic_call_info_by_index(pallet_index, call_index)
    }

    fn extrinsic_call_info_by_name(
        &self,
        pallet_name: &str,
        call_name: &str,
    ) -> Result<ExtrinsicCallInfo<'_, Self::TypeId>, ExtrinsicInfoError<'_>> {
        self.0.extrinsic_call_info_by_name(pallet_name, call_name)
    }

    fn extrinsic_signature_info(
        &self,
    ) -> Result<ExtrinsicSignatureInfo<Self::TypeId>, ExtrinsicInfoError<'_>> {
        self.0.extrinsic_signature_info()
    }

    fn extrinsic_extension_info(
        &self,
        extension_version: Option<u8>,
    ) -> Result<ExtrinsicExtensionInfo<'_, Self::TypeId>, ExtrinsicInfoError<'_>> {
        // The only change: a V4 extrinsic (`None`) always uses extension version 0.
        self.0
            .extrinsic_extension_info(Some(extension_version.unwrap_or(0)))
    }

    fn extrinsic_extension_version_info(
        &self,
    ) -> Result<impl Iterator<Item = u8>, ExtrinsicInfoError<'_>> {
        self.0.extrinsic_extension_version_info()
    }
}

/// Something went wrong decoding a block body entry.
#[derive(Debug, thiserror::Error)]
pub enum DecodeExtrinsicError {
    #[error(transparent)]
    Decode(#[from] ExtrinsicDecodeError),
    #[error("{leftover} bytes left over after decoding the extrinsic")]
    LeftoverBytes { leftover: usize },
}

/// Decode one block body entry into information about it.
///
/// `bytes` must be the entry as it appears in the block body, i.e. still carrying
/// its compact length prefix. Every range in the result is relative to `bytes`.
///
/// Callers that want to keep the bytes alongside the result should use
/// [`DecodedExtrinsic::decode`]; this exists so a caller can inspect the bytes
/// after a failure without having given up ownership of them.
pub fn decode_extrinsic_info(
    bytes: &[u8],
    metadata: &Metadata,
) -> Result<ExtrinsicOwned<u32>, DecodeExtrinsicError> {
    let compat = V4CompatMetadata(metadata);
    let cursor = &mut &bytes[..];
    let info = decode_extrinsic(cursor, &compat, metadata.types())?.into_owned();

    // Leftover bytes mean we misread the extrinsic even though every individual
    // part decoded, so treat it as a failure like subxt does.
    if !cursor.is_empty() {
        return Err(DecodeExtrinsicError::LeftoverBytes {
            leftover: cursor.len(),
        });
    }

    Ok(info)
}

/// A decoded extrinsic plus the bytes it was decoded from.
///
/// This mirrors the parts of `subxt::extrinsics::Extrinsic` that the block handlers
/// use. We can't use subxt's type directly because its decoding goes through
/// `Metadata`'s [`ExtrinsicTypeInfo`] implementation, which is what [`V4CompatMetadata`]
/// exists to correct.
pub struct DecodedExtrinsic {
    /// The block body entry, including its compact length prefix. All ranges in
    /// `info` are relative to these bytes.
    bytes: Vec<u8>,
    index: usize,
    info: ExtrinsicOwned<u32>,
    metadata: ArcMetadata,
}

impl DecodedExtrinsic {
    /// Decode one block body entry. `bytes` must be the entry as it appears in the
    /// block body, i.e. still carrying its compact length prefix.
    pub fn decode(
        index: usize,
        bytes: Vec<u8>,
        metadata: ArcMetadata,
    ) -> Result<Self, DecodeExtrinsicError> {
        let info = decode_extrinsic_info(&bytes, &metadata)?;

        Ok(Self::new(index, bytes, info, metadata))
    }

    /// Pair an already decoded [`ExtrinsicOwned`] with the bytes it was decoded
    /// from. The info must have come from [`decode_extrinsic_info`] called on
    /// exactly these bytes, or the ranges it holds will not line up.
    pub fn new(
        index: usize,
        bytes: Vec<u8>,
        info: ExtrinsicOwned<u32>,
        metadata: ArcMetadata,
    ) -> Self {
        Self {
            bytes,
            index,
            info,
            metadata,
        }
    }

    /// The index of this extrinsic within the block.
    pub fn index(&self) -> usize {
        self.index
    }

    /// The extrinsic bytes, including the compact length prefix that the block
    /// body entry carries.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Whether this is a V4 signed extrinsic.
    ///
    /// Note that a V5 `General` extrinsic reports `false` here even when it carries
    /// a signature, because that signature lives in a transaction extension.
    pub fn is_signed(&self) -> bool {
        self.info.is_signed()
    }

    pub fn pallet_name(&self) -> &str {
        self.info.pallet_name()
    }

    pub fn call_name(&self) -> &str {
        self.info.call_name()
    }

    /// The bytes of the address that signed this extrinsic, if it is signed.
    pub fn address_bytes(&self) -> Option<&[u8]> {
        self.info
            .signature_payload()
            .map(|s| &self.bytes[s.address_range()])
    }

    /// The signature bytes, if this extrinsic is signed.
    pub fn signature_bytes(&self) -> Option<&[u8]> {
        self.info
            .signature_payload()
            .map(|s| &self.bytes[s.signature_range()])
    }

    /// The scale encoded `extra` bytes of every transaction extension, in order.
    ///
    /// For V5 `General` extrinsics this excludes the extension version byte.
    pub fn transaction_extensions_bytes(&self) -> Option<&[u8]> {
        self.info
            .transaction_extension_payload()
            .map(|t| &self.bytes[t.range()])
    }

    /// The transaction extensions, if this extrinsic has any.
    pub fn transaction_extensions(&self) -> Option<TransactionExtensions<'_>> {
        self.info
            .transaction_extension_payload()
            .map(|payload| TransactionExtensions {
                bytes: &self.bytes,
                payload,
            })
    }

    /// Iterate over the fields of the call data.
    pub fn iter_call_data_fields(&self) -> impl Iterator<Item = CallDataField<'_>> {
        self.info.call_data().map(|field| CallDataField {
            bytes: &self.bytes[field.range()],
            name: field.name(),
            type_id: *field.ty(),
            types: self.metadata.types(),
        })
    }
}

/// The transaction extensions of a [`DecodedExtrinsic`].
pub struct TransactionExtensions<'a> {
    bytes: &'a [u8],
    payload: &'a ExtrinsicExtensions<'static, u32>,
}

impl<'a> TransactionExtensions<'a> {
    /// The transaction extension version these extensions were decoded with.
    pub fn version(&self) -> u8 {
        self.payload.version()
    }

    /// Iterate over each extension in the order the runtime declares them.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = TransactionExtension<'a>> + 'a {
        let bytes = self.bytes;
        self.payload.iter().map(move |arg| TransactionExtension {
            bytes: &bytes[arg.range()],
            name: arg.name(),
        })
    }
}

/// A single transaction extension.
pub struct TransactionExtension<'a> {
    bytes: &'a [u8],
    name: &'a str,
}

impl<'a> TransactionExtension<'a> {
    /// The extension's identifier as declared in the metadata.
    pub fn name(&self) -> &'a str {
        self.name
    }

    /// The scale encoded `extra` bytes for this extension.
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }
}

/// A field in the call data of a [`DecodedExtrinsic`].
pub struct CallDataField<'a> {
    bytes: &'a [u8],
    name: &'a str,
    type_id: u32,
    types: &'a PortableRegistry,
}

impl<'a> CallDataField<'a> {
    /// The field name, as declared in the metadata.
    pub fn name(&self) -> &'a str {
        self.name
    }

    /// The scale encoded bytes for this field.
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Decode this field into the given type.
    pub fn decode_as<T: scale_decode::DecodeAsType>(&self) -> Result<T, scale_decode::Error> {
        T::decode_as_type(&mut &*self.bytes, self.type_id, self.types)
    }

    /// Visit this field with the given visitor, returning its output.
    pub fn visit<V>(&self, visitor: V) -> Result<V::Value<'a, 'a>, V::Error>
    where
        V: scale_decode::visitor::Visitor<TypeResolver = PortableRegistry>,
    {
        scale_decode::visitor::decode_with_visitor(
            &mut &*self.bytes,
            self.type_id,
            self.types,
            visitor,
        )
    }
}

/// Fetch the raw block body: one entry per extrinsic, each still carrying the
/// compact length prefix that [`DecodedExtrinsic::decode`] expects.
///
/// We fetch the body ourselves rather than going through `subxt`'s
/// `extrinsics().fetch()` because that hands back already-decoded extrinsics and
/// keeps the raw bytes private, leaving no way to re-decode the ones it rejects.
/// This is the same `chain_getBlock` call subxt's legacy backend makes.
pub async fn fetch_block_body(
    legacy_rpc: &crate::state::SubstrateLegacyRpc,
    block_hash: subxt::utils::H256,
) -> Result<Vec<Vec<u8>>, subxt_rpcs::Error> {
    let Some(details) = legacy_rpc.chain_get_block(Some(block_hash)).await? else {
        return Ok(Vec::new());
    };

    Ok(details
        .block
        .extrinsics
        .into_iter()
        .map(|bytes| bytes.0)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_fixtures::ASSET_HUB_METADATA_V16;
    use frame_decode::extrinsics::ExtrinsicType;
    use parity_scale_codec::Decode;

    /// Polkadot Asset Hub block 20487777, extrinsic #2: a V4 signed
    /// `Balances.transfer_allow_death`, exactly as it appears in a
    /// `chain_getBlock` response (still compact-length-prefixed).
    const V4_TRANSFER_ALLOW_DEATH: &str = "0x59028400bf2f813dcbe2d0f9cb7b9dc61e46b2bcae7b6783adf184516229c891bc5359f500e8c8525ab31ec292d3762a583a93f2ca5f7b451185dd2c5a491e80e1fabf05235f83656132074cb9e6be03d7b25a40bd70af0861edfe7961ccb7d8894d0729007925152b025a620200000a00007a3584265306b5f6a6bd158f57e490c0f32d1135301b2fc89d7d8f76362af69007008824bf68";

    /// Polkadot Asset Hub block 20487777, extrinsic #3: a V4 signed
    /// `Balances.transfer_keep_alive`.
    const V4_TRANSFER_KEEP_ALIVE: &str = "0x55028400dc0c5e6f6c8265265f0bb9bbfa5c46a2471c05152066ed925e450361ad229913003132c42127d205558668f484efeafe4edcc8b49197e1f3f408774c411abc541d167c4f365bc62462bc2a46ca5db6284bf55bb48713171f15903e55cf7c415d01580529130000000a03001d46ccb04c50f93bde3457fd8e1dcee7da4ae2b8719f6d1df89f25afd75557c90f00506d96f51401";

    /// Polkadot Asset Hub block 20487777, extrinsic #1: a V5 `Bare`
    /// `Timestamp.set` inherent.
    const V5_BARE_TIMESTAMP_SET: &str = "0x280503000b80e44f8ba001";

    fn metadata() -> Metadata {
        let prefixed =
            frame_metadata::RuntimeMetadataPrefixed::decode(&mut &ASSET_HUB_METADATA_V16[..])
                .expect("fixture should be valid RuntimeMetadataPrefixed");
        Metadata::try_from(prefixed).expect("fixture should convert to subxt metadata")
    }

    fn bytes(hex_str: &str) -> Vec<u8> {
        hex::decode(hex_str.trim_start_matches("0x")).expect("valid hex")
    }

    /// Documents the runtime shape that triggers #405, so that a fixture swap
    /// which loses it can't quietly turn the tests below into no-ops.
    #[test]
    fn fixture_exposes_two_transaction_extension_versions() {
        let metadata = metadata();

        let mut versions: Vec<u8> = metadata
            .extrinsic_extension_version_info()
            .expect("versions available")
            .collect();
        versions.sort_unstable();
        assert_eq!(versions, vec![0, 1]);

        // Version 1 prepends extensions that version 0 doesn't have; decoding a V4
        // extrinsic against it is what fails.
        let v1_names: Vec<String> = metadata
            .extrinsic_extension_info(Some(1))
            .expect("version 1 available")
            .extension_ids
            .iter()
            .map(|e| e.name.to_string())
            .collect();
        assert_eq!(&v1_names[0], "UnitTransactionExtension");
        assert_eq!(&v1_names[1], "VerifyMultiSignature");
    }

    /// Pins the upstream behaviour we work around: `subxt_metadata::Metadata`
    /// answers "which extension version for a V4 extrinsic?" with the highest
    /// version in the metadata, and decoding then fails.
    ///
    /// See `paritytech/subxt#1998`. When that is fixed upstream and we bump to
    /// the release carrying the fix, this test starts failing, which is the signal
    /// that [`V4CompatMetadata`] can be removed.
    #[test]
    fn subxt_metadata_still_picks_the_newest_version_for_v4() {
        let metadata = metadata();

        assert_eq!(
            metadata
                .extrinsic()
                .transaction_extension_version_to_use_for_decoding(),
            1,
        );

        for hex_str in [V4_TRANSFER_ALLOW_DEATH, V4_TRANSFER_KEEP_ALIVE] {
            let raw = bytes(hex_str);
            let result = decode_extrinsic(&mut &raw[..], &metadata, metadata.types());
            assert!(
                result.is_err(),
                "expected the unfixed path to fail; if this now succeeds, subxt#1998 is fixed"
            );
        }
    }

    /// The fix: V4 extrinsics decode against extension version 0.
    #[test]
    fn v4_signed_extrinsics_decode_against_extension_version_0() {
        let metadata = metadata();

        let expected = [
            (V4_TRANSFER_ALLOW_DEATH, "transfer_allow_death"),
            (V4_TRANSFER_KEEP_ALIVE, "transfer_keep_alive"),
        ];

        for (hex_str, call_name) in expected {
            let raw = bytes(hex_str);
            let info = decode_extrinsic_info(&raw, &metadata)
                .unwrap_or_else(|e| panic!("{call_name} should decode: {e}"));

            assert_eq!(info.version(), 4);
            assert_eq!(info.ty(), ExtrinsicType::Signed);
            assert_eq!(info.pallet_name(), "Balances");
            assert_eq!(info.call_name(), call_name);
            assert!(info.is_signed());
            assert_eq!(
                info.transaction_extension_payload()
                    .expect("signed extrinsic has extensions")
                    .version(),
                0,
            );
        }
    }

    /// V5 extrinsics carry their own extension version, so the wrapper must not
    /// change how they decode.
    #[test]
    fn v5_extrinsics_are_unaffected() {
        let metadata = metadata();
        let raw = bytes(V5_BARE_TIMESTAMP_SET);

        let info = decode_extrinsic_info(&raw, &metadata).expect("V5 bare should decode");
        assert_eq!(info.version(), 5);
        assert_eq!(info.ty(), ExtrinsicType::Bare);
        assert_eq!(info.pallet_name(), "Timestamp");
        assert_eq!(info.call_name(), "set");
        assert!(!info.is_signed());
        assert!(info.transaction_extension_payload().is_none());

        // And it decodes identically through subxt's own (unfixed) path.
        let via_subxt = decode_extrinsic(&mut &raw[..], &metadata, metadata.types())
            .expect("V5 bare is not affected by the version selection");
        assert_eq!(via_subxt.call_name(), info.call_name());
    }

    /// The decoded ranges must line up with the bytes we hand back, since callers
    /// slice the extrinsic with them.
    #[test]
    fn decoded_ranges_line_up_with_the_extrinsic_bytes() {
        let metadata = std::sync::Arc::new(metadata());
        let raw = bytes(V4_TRANSFER_ALLOW_DEATH);

        let extrinsic = DecodedExtrinsic::decode(2, raw.clone(), metadata)
            .expect("V4 signed extrinsic should decode");

        assert_eq!(extrinsic.index(), 2);
        assert_eq!(extrinsic.bytes(), &raw[..]);

        // MultiAddress::Id(AccountId32) is a 1-byte variant index plus 32 bytes.
        let address = extrinsic.address_bytes().expect("signed");
        assert_eq!(address.len(), 33);
        assert_eq!(address[0], 0x00);

        // MultiSignature::Sr25519 is a 1-byte variant index plus 64 bytes.
        let signature = extrinsic.signature_bytes().expect("signed");
        assert_eq!(signature.len(), 65);

        let extensions = extrinsic
            .transaction_extensions()
            .expect("signed extrinsic has extensions");
        assert_eq!(extensions.version(), 0);

        let names: Vec<&str> = extensions.iter().map(|e| e.name()).collect();
        assert!(names.contains(&"CheckMortality"));
        assert!(names.contains(&"CheckNonce"));
        assert!(names.contains(&"ChargeAssetTxPayment"));

        // The extension `extra` bytes are a contiguous slice of the extrinsic, so
        // concatenating them must reproduce `transaction_extensions_bytes()`.
        let concatenated: Vec<u8> = extensions.iter().flat_map(|e| e.bytes().to_vec()).collect();
        assert_eq!(
            concatenated,
            extrinsic
                .transaction_extensions_bytes()
                .expect("signed")
                .to_vec()
        );

        let fields: Vec<&str> = extrinsic
            .iter_call_data_fields()
            .map(|f| f.name())
            .collect();
        assert_eq!(fields, vec!["dest", "value"]);
    }
}
