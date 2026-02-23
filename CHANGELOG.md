# Changelog

All notable changes to this project will be documented in this file.

See [standard-version](https://github.com/conventional-changelog/standard-version) for commit guidelines.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [0.1.0-beta.2] (2026-02-23)

Second beta release with improvements to error handling, relay chain connectivity, and developer experience.

### Features

- **JSON error responses for query parameters**: All query parameter validation errors now return structured JSON (`{"error": "..."}`) with 400 status instead of plain text. A new `JsonQuery<T>` extractor replaces `Query<T>` across all handlers. (#229)
- **Strict query parameter validation**: Unrecognized query parameters (e.g., typos like `onlyids` instead of `onlyIds`) now return a 400 error instead of being silently ignored. All query param structs enforce `deny_unknown_fields`. (#227)
- **Unified lazy relay chain initialization**: Consolidated two separate relay chain connection patterns into a single `OnceCell`-backed lazy-init approach. Handlers are simplified and errors are unified under a `RelayChainError` enum (`NotConfigured` → 400, `ConnectionFailed` → 503). (#230)
- **Relay chain reconnection support**: Multi-chain relay connections now support exponential backoff reconnection, reusing existing `SAS_SUBSTRATE_RECONNECT_*` environment variables. Connection failures at startup are now surfaced immediately. (#228)

### Fixes

- **Logging initialization order**: Logging is now initialized before `AppState` so that relay chain connection warnings are properly captured instead of silently lost. (#225)
- **Migration guide**: Added documentation for response differences in pallet metadata `args` field (simplified type names vs Sidecar's expanded definitions). (#234)

### Refactors

- **`resolve_client_at_block` helper**: Extracted common block resolution logic into a shared utility, reducing duplicated code per handler across 16+ handlers. (#226)

### Dependencies

- Bumped `keccak` from 0.1.5 to 0.1.6 (security fix). (#233)

## [1.0.0-beta.0] (2026-02-18)

First beta release of `polkadot-rest-api`, a REST service that makes it easy to interact with blockchain nodes built using Substrate's FRAME framework. This project is the successor to [substrate-api-sidecar](https://github.com/paritytech/substrate-api-sidecar), rewritten from the ground up in Rust.

### Highlights

- **Block endpoints**: Query blocks by number or hash, retrieve headers, extrinsics, events, and parachain inclusions. Supports `noFees`, `extrinsicDocs`, `eventDocs`, `decodeXcmMsgs`, and `useEvmFormat` query parameters.
- **Account endpoints**: Balance info, vesting info, staking info, staking payouts, proxy info, asset balances, foreign asset balances, pool asset balances, and asset/pool asset approvals.
- **Pallet endpoints**: Assets, foreign assets, pool assets, staking progress, staking validators, nomination pools, on-going referenda, asset conversion, storage, constants, dispatchables, errors, and events.
- **Transaction endpoints**: Submit transactions, estimate fees, dry-run, retrieve transaction material, and generate metadata blobs.
- **Runtime endpoints**: Spec, code (WASM blob), and metadata.
- **Coretime endpoints**: Info, overview, leases, regions, renewals, and reservations.
- **Parachain endpoints**: Parachain inclusions.
- **Node endpoints**: Version, roles, network, and transaction pool.
- **Relay chain mirror endpoints** (`/rc/*`): Query relay chain state for blocks, accounts, transactions, runtime, and node endpoints directly from an Asset Hub instance.
- **`useRcBlock` query parameter**: Map a relay chain block reference to the corresponding Asset Hub block, enabling cross-chain state queries.
- **OpenAPI documentation**: Custom UI available at `/docs` with interactive endpoint explorer. Auto-generated OpenAPI spec available at `/api-docs/openapi.json`.
- **Infrastructure**: Docker and docker-compose support, Prometheus metrics, Grafana dashboard, Loki log aggregation, WebSocket reconnection logic, and configurable HTTP logging.
- **SAS-compatible configuration**: Drop-in environment variable compatibility with substrate-api-sidecar (`SAS_SUBSTRATE_URL`, `SAS_EXPRESS_PORT`, etc.).

### Breaking Changes (vs substrate-api-sidecar)

- **URL prefix**: All endpoints are now versioned under `/v1` (e.g., `/blocks/head` → `/v1/blocks/head`).
- **`useRcBlockFormat` replaced by `format`**: Use `format=object` instead of `useRcBlockFormat=object`. The array format (default) no longer requires a parameter.
- **Coretime field renames**: `palletVersion` → `storageVersion` in `coretime/info`; `type` → `lifecycle` in `coretime/overview`.
- **Numeric fields**: All u16 and u32 fields are returned as numbers instead of strings. u128 values remain strings.
- **HTTP status codes**: Error responses now return 400 or 404 instead of 500 where appropriate.
- **Coretime price fix**: `currentCorePrice` calculation in `/v1/coretime/info` has been corrected.
- **Historical data with `?at=`**: Pallet endpoints (`assets`, `asset-conversion`, `pool-assets`, `foreign-assets`) now correctly return historical data when using the `?at=` query parameter. Sidecar returned current state regardless.
- **Removed endpoints**: Experimental/trace endpoints, ink! contract endpoints, and most parachain-specific endpoints (crowdloans, auctions, leases) are not implemented. See the [Migration Guide](docs/guides/MIGRATION.md) for the full list.
- **Removed config variables**: `SAS_SUBSTRATE_TYPES_*`, `SAS_SUBSTRATE_CACHE_CAPACITY`, `SAS_EXPRESS_INJECTED_CONTROLLERS`, and `SAS_LOG_FILTER_RPC` are no longer supported. `SAS_EXPRESS_MAX_BODY` is replaced by `SAS_EXPRESS_REQUEST_LIMIT`.
- **Runtime**: Requires a Rust binary instead of Node.js.

For a comprehensive migration guide, see [docs/guides/MIGRATION.md](docs/guides/MIGRATION.md).

### Compatibility

Tested against:
- Polkadot
- Kusama
- Westend
- Polkadot Asset Hub
- Kusama Asset Hub
