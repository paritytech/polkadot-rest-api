# Migration Guide


## Substrate API Sidecar v20.14.0 to Polkadot REST API
This guide documents breaking changes and differences between [substrate-api-sidecar](https://github.com/paritytech/substrate-api-sidecar) and this new Rust-based Polkadot REST API implementation.

## Overview

This project is a Rust-based alternative to substrate-api-sidecar, designed to provide improved performance, memory safety, and better resource utilization. While we aim to maintain API compatibility where possible, some breaking changes are necessary for architectural improvements.

---

## ⚠️ Breaking Changes ⚠️

### URL prefix

All API endpoints are now versioned under the `/v1` prefix.

| Sidecar | Polkadot REST API |
|---------|-------------------|
| `GET /blocks/head` | `GET /v1/blocks/head` |
| `GET /accounts/{id}/balance-info` | `GET /v1/accounts/{id}/balance-info` |
| `POST /transaction` | `POST /v1/transaction` |

Update all client URLs by prepending `/v1` to existing paths.

### Historical data with `?at=`

The following endpoints now return historical data when using the `?at=` query parameter. Sidecar's implementation returned current state regardless of the `at` parameter:
  - `/v1/pallets/assets/{assetId}/asset-info`
  - `/v1/pallets/asset-conversion/liquidity-pools`
  - `/v1/pallets/asset-conversion/next-available-id`
  - `/v1/pallets/pool-assets/{assetId}/asset-info`
  - `/v1/pallets/foreign-assets`

### Coretime endpoint changes

- **Renamed field**: `palletVersion` → `storageVersion` in `coretime/info`, to match current naming. See [commit](https://github.com/paritytech/polkadot-sdk/commit/4fe55f0bcb8edccaad73b33b804c349a756f7d3c).
- **Renamed field**: `type` → `lifecycle` in `coretime/overview`, to match the on-chain type (`ParaLifecycle` enum, `ParaLifecycles` storage, `fn lifecycle()` accessor).
- **Numeric fields**: All u16 and u32 fields are now returned as numbers instead of strings. This is an intentional divergence to provide more accurate JSON types, while maintaining safety for large values (u128 is still returned as a string).
- **HTTP status codes**: Error responses (e.g., pallet missing at a requested block) now return 400 or 404 instead of 500.
- **Price fix**: `/v1/coretime/info` — The `currentCorePrice` calculation has been corrected. The previous calculation was faulty. See [PR #175](https://github.com/paritytech/polkadot-rest-api/pull/175).

### Query parameter changes

| Parameter | Sidecar | Polkadot REST API | Notes |
|-----------|---------|-------------------|-------|
| `useRcBlockFormat` | Supported (`array` or `object`) | Not supported | Use `useRcBlock=true` only; response is always an array when multiple AH blocks found |

---

## Endpoints not available in Polkadot REST API

The following sidecar endpoints do **not** have equivalents in this project:

### Experimental / trace endpoints

| Sidecar endpoint | Status |
|------------------|--------|
| `GET /experimental/blocks/head/traces` | Not implemented |
| `GET /experimental/blocks/{blockId}/traces` | Not implemented |
| `GET /experimental/blocks/head/traces/operations` | Not implemented |
| `GET /experimental/blocks/{blockId}/traces/operations` | Not implemented |
| `GET /experimental/rc/blocks/head/traces` | Not implemented |
| `GET /experimental/rc/blocks/{blockId}/traces` | Not implemented |
| `GET /experimental/rc/blocks/head/traces/operations` | Not implemented |
| `GET /experimental/rc/blocks/{blockId}/traces/operations` | Not implemented |

### Ink! contracts

| Sidecar endpoint | Status |
|------------------|--------|
| `POST /contracts/ink/{address}/query` | Not implemented |

### Parachain endpoints

Most parachain-specific endpoints from sidecar are not implemented. Only parachain head inclusion is available:

| Sidecar endpoint | Polkadot REST API | Status |
|------------------|-------------------|--------|
| `GET /paras` | — | Not implemented |
| `GET /paras/{paraId}/crowdloan-info` | — | Not implemented (crowdloans deprecated) |
| `GET /paras/{paraId}/lease-info` | — | Not implemented |
| `GET /paras/auctions/current` | — | Not implemented (auctions deprecated) |
| `GET /paras/crowdloans` | — | Not implemented (crowdloans deprecated) |
| `GET /paras/head/backed-candidates` | — | Not implemented |
| `GET /paras/head/included-candidates` | — | Not implemented |
| `GET /paras/leases/current` | — | Not implemented |
| `GET /paras/{paraId}/head-inclusions` | `GET /v1/paras/{paraId}/head-inclusions` | Available |

> **Note:** Crowdloan and auction endpoints are deprecated on-chain (crowdloans have been superseded by Coretime). These are intentionally not ported.

---

## New endpoints (not in Sidecar)

| Endpoint | Description |
|----------|-------------|
| `GET /v1/version` | Returns the running Polkadot REST API version |
| `GET /v1/capabilities` | Returns supported pallets, chain type, and SS58 prefix |
| `GET /api-docs/openapi.json` | Auto-generated OpenAPI 3.0 spec |
| `GET /docs/` | Interactive documentation UI |

---

## Configuration changes

Both projects use environment variables with the `SAS_` prefix. Most variables are compatible.

### Supported in both

| Variable | Default | Purpose |
|----------|---------|---------|
| `SAS_SUBSTRATE_URL` | `ws://127.0.0.1:9944` | Primary RPC endpoint |
| `SAS_EXPRESS_PORT` | `8080` | HTTP server port |
| `SAS_EXPRESS_BIND_HOST` | `127.0.0.1` | Bind address |
| `SAS_EXPRESS_KEEP_ALIVE_TIMEOUT` | `5000` | Keep-alive timeout (ms) |
| `SAS_LOG_LEVEL` | `info` | Log level |
| `SAS_LOG_JSON` | `false` | JSON log output |
| `SAS_LOG_STRIP_ANSI` | `false` | Strip ANSI codes |
| `SAS_LOG_WRITE` | `false` | Write logs to file |
| `SAS_LOG_WRITE_PATH` | `./logs` | Log file directory |
| `SAS_LOG_WRITE_MAX_FILE_SIZE` | `5242880` | Max log file size |
| `SAS_LOG_WRITE_MAX_FILES` | `5` | Max number of log files |
| `SAS_METRICS_ENABLED` | `false` | Enable Prometheus metrics |
| `SAS_METRICS_PROM_HOST` | `127.0.0.1` | Prometheus host |
| `SAS_METRICS_PROM_PORT` | `9100` | Prometheus port |
| `SAS_METRICS_LOKI_HOST` | `127.0.0.1` | Loki host |
| `SAS_METRICS_LOKI_PORT` | `3100` | Loki port |
| `SAS_METRICS_INCLUDE_QUERYPARAMS` | `false` | Include query params in metric labels |
| `SAS_SUBSTRATE_MULTI_CHAIN_URL` | — | JSON array for relay chain connection |

### New in Polkadot REST API

| Variable | Default | Purpose |
|----------|---------|---------|
| `SAS_EXPRESS_BLOCK_FETCH_CONCURRENCY` | `10` | Concurrent block fetches |
| `SAS_EXPRESS_REQUEST_LIMIT` | `512000` | Max request body size (bytes) |
| `SAS_SUBSTRATE_RECONNECT_INITIAL_DELAY_MS` | `100` | RPC reconnect initial delay |
| `SAS_SUBSTRATE_RECONNECT_MAX_DELAY_MS` | `10000` | RPC reconnect max delay |
| `SAS_SUBSTRATE_RECONNECT_REQUEST_TIMEOUT_MS` | `30000` | RPC request timeout |
| `SAS_METRICS_PROMETHEUS_PREFIX` | `polkadot_rest_api` | Prometheus metric prefix |

### Sidecar-only (not supported)

| Variable | Purpose | Notes |
|----------|---------|-------|
| `SAS_SUBSTRATE_TYPES_BUNDLE` | Custom typesBundle | Not needed — subxt handles type resolution |
| `SAS_SUBSTRATE_TYPES_CHAIN` | Custom typesChain | Not needed |
| `SAS_SUBSTRATE_TYPES_SPEC` | Custom typesSpec | Not needed |
| `SAS_SUBSTRATE_TYPES` | Custom types | Not needed |
| `SAS_SUBSTRATE_CACHE_CAPACITY` | LRU cache size | Not implemented — uses different caching strategy |
| `SAS_EXPRESS_INJECTED_CONTROLLERS` | Pallet-injected controllers | Not supported |
| `SAS_EXPRESS_MAX_BODY` | Max body size | Replaced by `SAS_EXPRESS_REQUEST_LIMIT` |
| `SAS_LOG_FILTER_RPC` | Filter API-WS RPC logging | Not implemented |

---

## Installation changes

| | Sidecar | Polkadot REST API |
|---|---------|-------------------|
| **Runtime** | Node.js (v18+) | Rust binary (no runtime needed) |
| **Install** | `npm install @substrate/api-sidecar` | `cargo build --release --package server` |
| **Start** | `substrate-api-sidecar` | `cargo run --release --bin polkadot-rest-api` |
| **Docker** | `docker-compose up` | `docker-compose up` |

---

## Docker changes

Both projects provide a `Dockerfile` and `docker-compose.yml`. The compose stacks are similar (Polkadot node + API + Loki + Prometheus + Grafana + cAdvisor), but there are key differences.

### Image details

| | Sidecar | Polkadot REST API |
|---|---------|-------------------|
| **Base image (build)** | `node:18.12.1-alpine` | `rust:1.90.0-slim-bookworm` |
| **Base image (runtime)** | `node:18.12.1-alpine` | `debian:bookworm-slim` |
| **Runtime size** | Includes full Node.js runtime + `node_modules` | Static binary (~single executable) + `ca-certificates` |
| **Docker Hub** | `parity/substrate-api-sidecar` | `parity/polkadot-rest-api` |
| **Build command** | `yarn build:docker` | `docker build .` |
| **Run user** | `node` | `nobody` |
| **Default env vars** | `SAS_EXPRESS_PORT=8080`, `SAS_EXPRESS_BIND_HOST=0.0.0.0` | `SAS_EXPRESS_PORT=8080`, `SAS_EXPRESS_BIND_HOST=0.0.0.0`, `RUST_LOG=info` |

### Docker run

```bash
# Sidecar
docker pull docker.io/parity/substrate-api-sidecar:latest
docker run --rm -it --read-only -p 8080:8080 substrate-api-sidecar

# Polkadot REST API
docker pull docker.io/parity/polkadot-rest-api:latest
docker run --rm -it --read-only -p 8080:8080 polkadot-rest-api
```

Both images support the `--read-only` flag and accept environment variables via `-e` or `--env-file`.

### docker-compose.yml differences

Both compose files include the same services: `polkadot` node, API server, `loki`, `cadvisor`, `prometheus`, and `grafana`. Key differences:

| | Sidecar | Polkadot REST API |
|---|---------|-------------------|
| **API service name** | `sidecar` | `rest-api` |
| **Exposed ports** | `8080:8080`, `9100:9100` | `8080:8080` |
| **Bind host config** | Set via env var `SAS_EXPRESS_BIND_HOST: "0.0.0.0"` | Set in Dockerfile default (`0.0.0.0`) |
| **Prometheus metrics host** | Set via env var `SAS_METRICS_PROM_HOST: "0.0.0.0"` | Not set (uses default `127.0.0.1`) |

### Migration steps for Docker users

1. **Update image name**: Replace `parity/substrate-api-sidecar` with `parity/polkadot-rest-api`
2. **Update service name** (if referenced): `sidecar` → `rest-api` (or your preferred name)
3. **Remove `SAS_EXPRESS_BIND_HOST`** from environment if set to `0.0.0.0` — this is now the Dockerfile default
4. **Update health checks**: Change `curl http://localhost:8080/blocks/head` to `curl http://localhost:8080/v1/blocks/head` (note the `/v1` prefix)
5. **Remove unused env vars**: `SAS_SUBSTRATE_TYPES_*`, `SAS_SUBSTRATE_CACHE_CAPACITY`, `SAS_EXPRESS_INJECTED_CONTROLLERS` are not supported
6. **Prometheus port**: Sidecar exposed port `9100` for Prometheus metrics. Polkadot REST API serves metrics on the main port at `/metrics` — update your Prometheus scrape config accordingly

---

## API Changes

- `/v1/version` - Now users can query the currently running version of Polkadot REST API
