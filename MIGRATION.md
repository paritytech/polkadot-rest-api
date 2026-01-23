# Migration Guide


## Substrate API Sidecar v20.9.1 to Polkadot REST API
This guide documents breaking changes and differences between [substrate-api-sidecar](https://github.com/paritytech/substrate-api-sidecar) and this new Rust-based Polkadot REST API implementation.

## Overview

This project is a Rust-based alternative to substrate-api-sidecar, designed to provide improved performance, memory safety, and better resource utilization. While we aim to maintain API compatibility where possible, some breaking changes are necessary for architectural improvements.

### ⚠️ Breaking Changes ⚠️

- All API endpoints are now versioned under the `/v1` prefix.
- The following endpoints now return historical data when using the `?at=` query parameter. Sidecar's implementation returned current state regardless of the `at` parameter:
  - `/v1/pallets/assets/{assetId}/asset-info`
  - `/v1/pallets/asset-conversion/liquidity-pools`
  - `/v1/pallets/asset-conversion/next-available-id`

### API Changes

- `/v1/version` - Now users can query the currently running version of Polkadot REST API