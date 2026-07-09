---
name: v5-extrinsic-signatures
description: Use when extrinsics unexpectedly report is_signed() == false or a missing signature, or when handling V5 "General" extrinsics/transactions
---

# V5 extrinsic signatures live in transaction extensions

## Overview

Subxt decodes V5 extrinsics fine, but V5 General transactions **appear unsigned**: `extrinsic.signature_bytes() == None` and `extrinsic.is_signed() == false`. This is not a bug — the signature moved into the transaction extensions.

## Finding the signature

Look for the `VerifySignature` transaction extension:

```rust
use subxt::config::transaction_extensions::VerifySignature;

let details = extrinsic
    .transaction_extensions()
    .and_then(|exts| exts.find::<VerifySignature<_>>()) // config is inferred from the extrinsic
    .transpose()?; // find returns Option<Result<_>>: Some(Err(_)) = matched but undecodable, None = no match
// details: Option<VerifySignatureDetails> — Disabled, or Signed { signature, account }
```

In this repo the client is `SubstrateConfig` (`crates/server/src/state.rs`), so leave the config as `VerifySignature<_>` and let it infer — a hardcoded `VerifySignature<PolkadotConfig>` won't satisfy `find`'s bound. Put the lookup alongside the other shared extension types in `crates/server/src/utils/transaction_extensions.rs` rather than hand-rolling it per handler.

## Version landmine

The on-chain identifier the extension matches against changed:

| subxt version | identifier matched |
|---|---|
| `< 0.50.1` (the beta.4 in `crates/server/Cargo.toml`) | `"VerifySignature"` |
| `0.50.1+` | `"VerifyMultiSignature"` (rename, subxt#2197) |

The Rust type is unchanged — you keep writing `find::<VerifySignature<_>>()` on both sides of the bump. What changed is the extension **name in chain metadata** that the lookup matches: `< 0.50.1` only finds runtimes calling it `VerifySignature`; `0.50.1+` only finds runtimes calling it `VerifyMultiSignature`. A `find` name mismatch silently returns `None`.

You don't have to change subxt versions to fix it — match either name yourself:

```rust
let sig = match extrinsic.transaction_extensions() {
    Some(exts) => exts
        .iter()
        .find(|ext| matches!(ext.name(), "VerifySignature" | "VerifyMultiSignature"))
        .map(|ext| ext.decode_unchecked_as::<VerifySignatureDetails<SubstrateConfig>>())
        .transpose()?,
    None => None,
};
```

`decode_unchecked_as` skips the name check that `find` enforces, so one path handles both identifiers. Unlike `find` (which infers the config from the extrinsic), `decode_unchecked_as` can't infer it — name the config explicitly (`SubstrateConfig` in this repo).

## Caveat

Chains can opt to use different extensions or handle signatures in arbitrary ways under V5 — don't assume `VerifySignature` is universal across chains.
