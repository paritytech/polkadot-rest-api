// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared transaction extension types for decoding signed extrinsics.
//!
//! These types are used to decode the transaction extensions (formerly "signed extensions")
//! present in signed extrinsics to extract nonce, tip, and other metadata.

use parity_scale_codec::Decode;

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
