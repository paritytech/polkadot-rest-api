# Migration Guide


## Substrate API Sidecar v20.14.0 to Polkadot REST API
This guide documents breaking changes and differences between [substrate-api-sidecar](https://github.com/paritytech/substrate-api-sidecar) and this new Rust-based Polkadot REST API implementation.

## Overview

This project is a Rust-based alternative to substrate-api-sidecar, designed to provide improved performance, memory safety, and better resource utilization. While we aim to maintain API compatibility where possible, some breaking changes are necessary for architectural improvements.

### ⚠️ Breaking Changes ⚠️

- All API endpoints are now versioned under the `/v1` prefix.
- The following endpoints now return historical data when using the `?at=` query parameter. Sidecar's implementation returned current state regardless of the `at` parameter:
  - `/v1/pallets/assets/{assetId}/asset-info`
  - `/v1/pallets/asset-conversion/liquidity-pools`
  - `/v1/pallets/asset-conversion/next-available-id`
  - `/v1/pallets/pool-assets/{assetId}/asset-info`
  - `/v1/pallets/foreign-assets`
- Renamed `palletVersion` to `storageVersion` in `coretime/info`, in order to match current naming. For more context, see the linked [commit](https://github.com/paritytech/polkadot-sdk/commit/4fe55f0bcb8edccaad73b33b804c349a756f7d3c).

#### Coretime endpoints
- Numeric Fields: All u16 and u32 fields are now returned as numbers instead of strings. This is an intentional divergence to provide more accurate JSON types, while maintaining safety for large values (u128 is still returned as a string).
- HTTP Status Codes: Error responses that occur when, for example, a pallet is missing at a requested block now return 400 or 404 instead of 500.
- `/v1/coretime/info`: The `currentCorePrice` calculation has been corrected. The previous calculation was faulty. For more details, see the related [PR](https://github.com/paritytech/polkadot-rest-api/pull/175).
- `/v1/coretime/overview`: The `type` field is now returned as `lifecycle` since this is a more accurate name field that matches the on-chain type (`ParaLifecycle` enum, `ParaLifecycles` storage, `fn lifecycle()` accessor).


### API Changes

- `/v1/version` - Now users can query the currently running version of Polkadot REST API