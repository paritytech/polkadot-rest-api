---
name: prefer-subxt-over-rpc
description: Use when fetching or decoding chain data (blocks, extrinsics, events, storage, runtime APIs) and tempted to make manual RPC calls (legacy_rpc, rpc_params!, chain_getBlock, state_call), or when writing code that branches on metadata versions
---

# Prefer Subxt over manual RPC

## Overview

Subxt 0.50 covers ~99% of chain access — fetching and decoding blocks, extrinsics, events, storage, constants, runtime APIs and view functions — across **all runtime versions back to genesis**. Manual RPC (`legacy_rpc`, `rpc_params!`) is a last resort, not a default.

## Quick reference

| Task | Instead of RPC, use |
|------|---------------------|
| Latest finalized block | `api.at_current_block().await?` |
| Specific block | `api.at_block(...).await?` |
| Storage read (one-shot) | `at_block.storage().fetch(addr, keys)` / `.iter(...)` |
| Storage read (reusable handle) | `at_block.storage().entry(addr)?` then `.fetch(keys)` |
| Storage read that may be absent | `at_block.storage().try_fetch(addr, keys)` (returns `Option`) |
| Events | `at_block.events()` |
| Extrinsics | `at_block.extrinsics()` |
| Runtime API call | `at_block.runtime_apis()` |
| View functions | `at_block.view_functions()` |
| Constants | `at_block.constants()` |
| Metadata / spec version | `at_block.metadata()`, `at_block.spec_version()` |

End-to-end example — a dynamic storage read at a block, no RPC anywhere:

```rust
let at_block = api.at_block(block_hash).await?;
let addr = subxt::dynamic::storage::<(), u32>("Referenda", "ReferendumCount");
let count: u32 = at_block.storage().fetch(addr, ()).await?.decode()?;
```

Keyed entries take the keys as a tuple in `fetch` (e.g. `.fetch(addr, (account_id,))`). Both `fetch` and `entry` accept dynamic and static/codegen addresses alike — `fetch(addr, keys)` is just shorthand for `entry(addr)?.fetch(keys)`, so choose by whether you reuse the handle, not by address kind. Note `fetch` **errors** with `NoValueFound` when a map entry has no value and no default; for entries that may be absent (common on account/map reads) use `try_fetch`, which returns `Option`. See the `scale-decoding` skill for choosing the value type parameter.

## Metadata versions: don't hand-roll (for chain data)

When **reading chain data** (storage, events, extrinsics, runtime APIs), don't branch on metadata versions yourself. Subxt already converts **every historic and modern metadata format** into `subxt::Metadata`, and has usually already downloaded it for the block you're working at.

Pre-V14 blocks additionally need legacy type info:
- `PolkadotConfig` ships Polkadot relay chain historic types **by default** (opt out with `PolkadotConfig::builder().use_historic_types(false).build()`).
- `SubstrateConfig` needs them provided: `SubstrateConfig::builder().set_legacy_types(registry).build()` (a `scale_info_legacy::ChainTypeRegistry`). This repo uses `SubstrateConfig`, and that wiring already lives in `build_subxt_config` (`crates/server/src/state.rs`), keyed by the chain-config `legacy_types` string — add a case there rather than building configs inline.

For chain-data access, if `subxt::Metadata` doesn't expose something you need, file a subxt PR to expose it rather than decoding raw metadata.

**Exception:** endpoints that serve version-faithful raw metadata JSON (e.g. `handlers/runtime/get_metadata.rs`, which emits distinct `v14`/`v15` shapes) legitimately decode `frame_metadata::RuntimeMetadata` and branch on its version — normalized `subxt::Metadata` can't reproduce those shapes. That branching is expected there; don't "fix" it away.

## Notes

- No runtime-upgrade monitoring is needed — subxt resolves the correct metadata/spec version per block automatically. Mind the cost, though: with the default config (this repo sets no spec-version ranges), each `at_block` issues an extra `Core_version` RPC to resolve the spec version — converted metadata is cached per spec version, but that resolution is not cached per block. On hot paths, pre-seed with `set_spec_version_for_block_ranges` / `set_metadata_for_spec_versions` to drop the round-trips.
- The subxt pin lives in `crates/server/Cargo.toml` (likewise `frame-decode` and its `legacy-types` feature). Before relying on version-specific behavior, check that pin against the subxt CHANGELOG — e.g. `SubstrateConfigBuilder::set_genesis_hash` was a no-op until 0.50.2. The `VerifySignature` extension-identifier rename is covered in the `v5-extrinsic-signatures` skill.
