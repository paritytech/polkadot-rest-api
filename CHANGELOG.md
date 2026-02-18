# Changelog

All notable changes to this project will be documented in this file.

See [standard-version](https://github.com/conventional-changelog/standard-version) for commit guidelines.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

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
- **OpenAPI documentation**: Auto-generated Swagger UI available at the root endpoint.
- **Infrastructure**: Docker and docker-compose support, Prometheus metrics, Grafana dashboard, Loki log aggregation, WebSocket reconnection logic, and configurable HTTP logging.
- **SAS-compatible configuration**: Drop-in environment variable compatibility with substrate-api-sidecar (`SAS_SUBSTRATE_URL`, `SAS_EXPRESS_PORT`, etc.).

### Compatibility

Tested against:
- Polkadot
- Kusama
- Westend
- Polkadot Asset Hub
- Kusama Asset Hub
