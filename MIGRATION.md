# Migration Guide# Migration Guide





## Substrate API Sidecar v20.9.1 to Polkadot REST API## Substrate API Sidecar v20.9.1 to Polkadot REST API

This guide documents breaking changes and differences between [substrate-api-sidecar](https://github.com/paritytech/substrate-api-sidecar) and this new Rust-based Polkadot REST API implementation.This guide documents breaking changes and differences between [substrate-api-sidecar](https://github.com/paritytech/substrate-api-sidecar) and this new Rust-based Polkadot REST API implementation.



## Overview## Overview



This project is a Rust-based alternative to substrate-api-sidecar, designed to provide improved performance, memory safety, and better resource utilization. While we aim to maintain API compatibility where possible, some breaking changes are necessary for architectural improvements.This project is a Rust-based alternative to substrate-api-sidecar, designed to provide improved performance, memory safety, and better resource utilization. While we aim to maintain API compatibility where possible, some breaking changes are necessary for architectural improvements.



### ⚠️ Breaking Changes ⚠️### ⚠️ Breaking Changes ⚠️



- All API endpoints are now versioned under the `/v1` prefix.- All API endpoints are now versioned under the `/v1` prefix.

- Type references in metadata responses use type names (e.g., `"U32"`) instead of type IDs (e.g., `42`)<<<<<<< HEAD

- `/v1/pallets/assets/{assetId}/asset-info` - Now returns historical data when using the `?at=` query parameter. Sidecar's implementation returned current state regardless of the `at` parameter.- Type references in metadata responses use type names (e.g., `"U32"`) instead of type IDs (e.g., `42`)

=======

### API Changes- `/v1/pallets/assets/{assetId}/asset-info` - Now returns historical data when using the `?at=` query parameter. Sidecar's implementation returned current state regardless of the `at` parameter.

>>>>>>> origin/main

- `/v1/version` - Now users can query the currently running version of Polkadot REST API

### API Changes

- `/v1/version` - Now users can query the currently running version of Polkadot REST API