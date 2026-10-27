// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared transaction extension types for decoding signed extrinsics.
//!
//! These types are used to decode the transaction extensions (formerly "signed extensions")
//! present in signed extrinsics to extract nonce, tip, and other metadata.

use crate::utils::extrinsic_decode::TransactionExtensions;
use parity_scale_codec::Decode;
use subxt::SubstrateConfig;
use subxt::config::transaction_extensions::VerifySignatureDetails;
use subxt::utils::MultiSignature;

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

/// The signature a v5 `General` extrinsic carries, pulled out of its transaction
/// extensions.
pub struct GeneralSignature {
    /// Signature bytes with the `MultiSignature` variant byte already stripped, so this
    /// reads the same as what a v4 signed extrinsic reports.
    pub signature: Vec<u8>,
    /// The account that produced the signature.
    pub account: [u8; 32],
}

/// Whether an extension name is the authorization extension that carries a v5 signature.
///
/// Both spellings are matched because the on chain identifier was renamed from
/// `VerifySignature` to `VerifyMultiSignature` in subxt 0.50.1 while the Rust type kept
/// its name. A name mismatch here does not error, it just silently yields no signature.
pub fn is_authorization_extension(name: &str) -> bool {
    matches!(name, "VerifySignature" | "VerifyMultiSignature")
}

/// Decode an authorization extension's bytes into the signature it carries.
///
/// Driven by the type the metadata declares for the extension rather than by a layout
/// assumed here, so a runtime that orders or names the variants differently still decodes.
pub fn decode_general_signature(
    bytes: &[u8],
    type_id: u32,
    types: &scale_info::PortableRegistry,
) -> Option<GeneralSignature> {
    use scale_decode::DecodeAsType;

    match VerifySignatureDetails::<SubstrateConfig>::decode_as_type(&mut &*bytes, type_id, types) {
        Ok(VerifySignatureDetails::Signed { signature, account }) => Some(GeneralSignature {
            signature: multi_signature_bytes(&signature),
            account: account.0,
        }),
        Ok(VerifySignatureDetails::Disabled) => None,
        Err(e) => {
            // A chain whose signature type we cannot represent, an `Eth` variant say, lands
            // here. Report the extrinsic as unsigned rather than failing over a field we
            // could not read.
            tracing::warn!(
                error = %e,
                "Could not decode the authorization extension; reporting the extrinsic as unsigned"
            );
            None
        }
    }
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

/// The raw signature bytes, without the variant byte that says which scheme produced them.
///
/// v4 reports its signature with that prefix stripped, and the two formats should not
/// differ just because of where the signature was found.
fn multi_signature_bytes(signature: &MultiSignature) -> Vec<u8> {
    match signature {
        MultiSignature::Ed25519(bytes) | MultiSignature::Sr25519(bytes) => bytes.to_vec(),
        MultiSignature::Ecdsa(bytes) => bytes.to_vec(),
    }
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
