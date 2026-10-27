// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared transaction extension types for decoding signed extrinsics.
//!
//! These types are used to decode the transaction extensions (formerly "signed extensions")
//! present in signed extrinsics to extract nonce, tip, and other metadata.

use crate::utils::extrinsic_decode::TransactionExtensions;
use parity_scale_codec::Decode;
use scale_decode::DecodeAsType;
use scale_value::{Composite, Value, ValueDef};
use subxt::utils::AccountId32;

/// CheckNonce signed extension - contains the account nonce.
///
/// SCALE encoded as a compact u32.
#[derive(Decode)]
pub struct CheckNonce(#[codec(compact)] pub u32);

/// ChargeTransactionPayment signed extension - contains the tip amount.
///
/// SCALE encoded as a compact u128.
#[derive(Decode)]
pub struct ChargeTransactionPayment(#[codec(compact)] pub u128);

/// ChargeAssetTxPayment signed extension - contains tip and optional asset_id.
///
/// Used on Asset Hub and other chains that support paying fees in non-native assets.
#[derive(Decode)]
pub struct ChargeAssetTxPayment {
    #[codec(compact)]
    pub tip: u128,
    // asset_id is Option<AssetId> but we don't need it for tip extraction
}

/// Whether an extension name is the authorization extension that carries a v5 signature.
///
/// Both spellings are matched because subxt renamed the identifier it looks for from
/// `VerifySignature` to `VerifyMultiSignature` in 0.50.1 while keeping the Rust type's name.
/// The on chain identifier itself comes from polkadot-sdk's `pallet_verify_signature`, and
/// matching both costs nothing if only one was ever published.
///
/// Note this is not the only authorization extension a runtime can declare; Asset Hub also
/// declares `AuthorizeCall`. An extrinsic authorized through a different one has no
/// signature here and is reported unsigned, which is accurate: there is no signature to
/// report.
pub fn is_authorization_extension(name: &str) -> bool {
    matches!(name, "VerifySignature" | "VerifyMultiSignature")
}

/// The authorization extension as the runtime declares it.
///
/// Decoded against the metadata's own type, so the variant indices and field order on chain
/// decide the layout rather than the order written here. That is load bearing: Asset Hub
/// numbers `Signed` as variant 1, so a decode that trusted this declaration order would read
/// a real signature as `Disabled` and report no signature at all.
#[derive(DecodeAsType)]
enum AuthorizationExtension {
    Signed {
        /// Kept opaque rather than typed as a `MultiSignature`, see [`signature_bytes`].
        signature: Value<()>,
        account: AccountId32,
    },
    Disabled,
}

/// The signature bytes, with the leading byte that names the signing scheme dropped.
///
/// The v4 path reports `signature_bytes()[1..]`: the encoded signature minus its scheme byte,
/// with no interest in which scheme it was. Taking the signature opaquely here does the same,
/// so the two report identically and, more importantly, a scheme we have no Rust type for
/// still decodes. That is not hypothetical. `subxt::utils::MultiSignature` knows only
/// Ed25519, Sr25519 and Ecdsa, while Asset Hub's own `MultiSignature` declares `Eth` as a
/// fourth, and decoding into an enumerated type turns any scheme missing from it into a
/// silently unsigned extrinsic, which is the exact failure this module exists to fix.
fn signature_bytes(signature: &Value<()>) -> Option<Vec<u8>> {
    // A signature is an enum over schemes; the scheme is the variant and the bytes are its
    // single field. The variant name is deliberately not inspected.
    let ValueDef::Variant(variant) = &signature.value else {
        return None;
    };

    let fields: Vec<&Value<()>> = variant.values.values().collect();
    let [only] = fields[..] else {
        return None;
    };

    let bytes = match &only.value {
        // The usual shape: one field holding a `[u8; N]`, which renders as a composite of u8.
        ValueDef::Composite(inner) => inner,
        // A scheme declaring its bytes as the variant's fields directly.
        _ => &variant.values,
    };

    collect_bytes(bytes)
}

/// Collect a composite of integers into bytes, failing on anything that is not a byte.
fn collect_bytes(composite: &Composite<()>) -> Option<Vec<u8>> {
    composite
        .values()
        .map(|v| u8::try_from(v.as_u128()?).ok())
        .collect()
}

/// Decode an authorization extension's bytes into the signature it carries.
pub fn decode_general_signature(
    bytes: &[u8],
    type_id: u32,
    types: &scale_info::PortableRegistry,
) -> Option<GeneralSignature> {
    let cursor = &mut &*bytes;

    let decoded = match AuthorizationExtension::decode_as_type(cursor, type_id, types) {
        Ok(decoded) => decoded,
        Err(e) => {
            tracing::warn!(
                error = %e,
                "Could not decode the authorization extension; reporting the extrinsic as unsigned"
            );
            return None;
        }
    };

    // Trailing bytes mean this is not the type we think it is, so the signature we just read
    // is not trustworthy either.
    if !cursor.is_empty() {
        tracing::warn!(
            leftover = cursor.len(),
            "Authorization extension had trailing bytes; reporting the extrinsic as unsigned"
        );
        return None;
    }

    match decoded {
        AuthorizationExtension::Signed { signature, account } => Some(GeneralSignature {
            signature: signature_bytes(&signature)?,
            account: account.0,
        }),
        AuthorizationExtension::Disabled => None,
    }
}

/// The signature a v5 `General` extrinsic carries, pulled out of its transaction
/// extensions.
pub struct GeneralSignature {
    /// Signature bytes with the scheme byte already stripped, so this reads the same as what
    /// a v4 signed extrinsic reports.
    pub signature: Vec<u8>,
    /// The account that produced the signature.
    pub account: [u8; 32],
}

/// Find the signature of a v5 `General` extrinsic.
///
/// A v5 `General` extrinsic has no signature field, so subxt reports it as unsigned. The
/// signature is instead carried by an authorization extension, see
/// [`is_authorization_extension`].
///
/// Returns `None` for a v4 extrinsic (no such extension), for a genuinely unsigned one,
/// and for `Disabled`, which is the extension's way of saying no signature was provided.
pub fn general_signature(extensions: &TransactionExtensions<'_>) -> Option<GeneralSignature> {
    let ext = extensions
        .iter()
        .find(|ext| is_authorization_extension(ext.name()))?;

    ext.decode_general_signature()
}

#[cfg(test)]
mod tests {
    use super::*;
    use parity_scale_codec::{Compact, Encode};

    #[test]
    fn test_check_nonce_decode() {
        // CheckNonce expects a compact-encoded u32
        let encoded = Compact(42u32).encode();
        let decoded = CheckNonce::decode(&mut &encoded[..]).unwrap();
        assert_eq!(decoded.0, 42);
    }

    #[test]
    fn test_charge_transaction_payment_decode() {
        // ChargeTransactionPayment expects a compact-encoded u128
        let encoded = Compact(1000u128).encode();
        let decoded = ChargeTransactionPayment::decode(&mut &encoded[..]).unwrap();
        assert_eq!(decoded.0, 1000);
    }

    #[test]
    fn test_charge_asset_tx_payment_decode() {
        // ChargeAssetTxPayment expects compact tip - just verify compact decoding works
        let tip: u128 = 500;
        let encoded = Compact(tip).encode();
        let decoded_tip = Compact::<u128>::decode(&mut &encoded[..]).unwrap();
        assert_eq!(decoded_tip.0, 500);
    }
}
