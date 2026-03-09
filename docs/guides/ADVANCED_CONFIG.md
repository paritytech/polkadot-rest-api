# Advanced Configuration Guide

This comprehensive guide covers all configuration options available in Polkadot REST API. All environment variables use the `SAS_` prefix for backwards compatibility with Substrate API Sidecar.

## Table of Contents

- [Server Configuration](#server-configuration)
- [Substrate Node Connection](#substrate-node-connection)
- [Logging Configuration](#logging-configuration)
- [Metrics & Monitoring](#metrics--monitoring)
- [Environment Profiles](#environment-profiles)
- [Docker Configuration](#docker-configuration)

## Server Configuration

Configure the HTTP server that serves the REST API.

### Basic Server Settings

| Variable | Default | Description |
|----------|---------|-------------|
| `SAS_EXPRESS_BIND_HOST` | `127.0.0.1` | Network interface to bind to. **Use `0.0.0.0` for Docker** |
| `SAS_EXPRESS_PORT` | `8080` | Port number (must be non-zero) |
| `SAS_EXPRESS_KEEP_ALIVE_TIMEOUT` | `5000` | Keep-alive timeout in milliseconds |
| `SAS_EXPRESS_REQUEST_LIMIT` | `512000` | Maximum request body size in bytes (500KB) |
| `SAS_EXPRESS_BLOCK_FETCH_CONCURRENCY` | `10` | Maximum concurrent block fetches for block range queries |

> **Note:** The `SAS_EXPRESS_` prefix is an artifact of Substrate API Sidecar naming, preserved for backwards compatibility.

**Example:**
```bash
export SAS_EXPRESS_BIND_HOST=0.0.0.0
export SAS_EXPRESS_PORT=3000
export SAS_EXPRESS_KEEP_ALIVE_TIMEOUT=10000
export SAS_EXPRESS_REQUEST_LIMIT=1048576    # 1MB
export SAS_EXPRESS_BLOCK_FETCH_CONCURRENCY=20
```

### Migrating from Sidecar

The following sidecar server variables are **not supported**:

| Sidecar Variable | Status | Notes |
|------------------|--------|-------|
| `SAS_EXPRESS_MAX_BODY` | Replaced | Use `SAS_EXPRESS_REQUEST_LIMIT` (value in bytes, not a string like `100kb`) |
| `SAS_EXPRESS_INJECTED_CONTROLLERS` | Not supported | Pallet-injected controllers are not available |

## Substrate Node Connection

Configure connections to Substrate-based blockchain nodes.

### Primary Node Connection

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `SAS_SUBSTRATE_URL` | Yes | `ws://127.0.0.1:9944` | WebSocket or HTTP URL to node |

**Supported protocols:** `ws://`, `wss://`, `http://`, `https://`

### Reconnection Settings

| Variable | Default | Description |
|----------|---------|-------------|
| `SAS_SUBSTRATE_RECONNECT_INITIAL_DELAY_MS` | `100` | Initial delay before first reconnect attempt |
| `SAS_SUBSTRATE_RECONNECT_MAX_DELAY_MS` | `10000` | Maximum delay between reconnect attempts (10s) |
| `SAS_SUBSTRATE_RECONNECT_REQUEST_TIMEOUT_MS` | `30000` | Timeout for individual RPC requests (30s) |

### Multi-Chain Configuration

For Asset Hub and multi-chain queries (e.g., `useRcBlock` functionality):

| Variable | Required | Description |
|----------|----------|-------------|
| `SAS_SUBSTRATE_MULTI_CHAIN_URL` | No | JSON array of chain configurations |

**Format:**
```json
[{"url":"wss://relay-chain.com","type":"relay"}]
```

**Chain types:**
- `relay` — Relay chain (Polkadot/Kusama)
- `assethub` — Asset Hub parachain
- `coretime` — Coretime chain
- `parachain` — Other parachains (default)

**Example configurations:**

```bash
# Single node (basic)
export SAS_SUBSTRATE_URL=wss://polkadot-rpc.polkadot.io

# Asset Hub with relay chain (enables useRcBlock and RC endpoints)
export SAS_SUBSTRATE_URL=wss://polkadot-asset-hub-rpc.polkadot.io
export SAS_SUBSTRATE_MULTI_CHAIN_URL='[{"url":"wss://polkadot-rpc.polkadot.io","type":"relay"}]'

# Local development
export SAS_SUBSTRATE_URL=ws://127.0.0.1:9944
export SAS_SUBSTRATE_MULTI_CHAIN_URL='[{"url":"ws://127.0.0.1:9945","type":"relay"}]'
```

### Migrating from Sidecar

The following sidecar substrate variables are **not supported**:

| Sidecar Variable | Status | Notes |
|------------------|--------|-------|
| `SAS_SUBSTRATE_TYPES_BUNDLE` | Not needed | subxt handles type resolution automatically |
| `SAS_SUBSTRATE_TYPES_CHAIN` | Not needed | subxt handles type resolution automatically |
| `SAS_SUBSTRATE_TYPES_SPEC` | Not needed | subxt handles type resolution automatically |
| `SAS_SUBSTRATE_TYPES` | Not needed | subxt handles type resolution automatically |
| `SAS_SUBSTRATE_CACHE_CAPACITY` | Not implemented | Uses a different caching strategy |

## Logging Configuration

Control logging behavior and output formatting.

### Log Levels

| Variable | Default | Options | Description |
|----------|---------|---------|-------------|
| `SAS_LOG_LEVEL` | `info` | `error`, `warn`, `info`, `http`, `debug`, `trace` | Minimum log level |

**HTTP Status Code Mapping:**
- `< 400` — `http` level
- `400-499` — `warn` level
- `>= 500` — `error` level

### Log Formatting

| Variable | Default | Description |
|----------|---------|-------------|
| `SAS_LOG_JSON` | `false` | Output logs in JSON format |
| `SAS_LOG_STRIP_ANSI` | `false` | Remove ANSI color codes |

### File Logging

| Variable | Default | Description |
|----------|---------|-------------|
| `SAS_LOG_WRITE` | `false` | Write logs to file |
| `SAS_LOG_WRITE_PATH` | `./logs` | Directory for log files |
| `SAS_LOG_WRITE_MAX_FILE_SIZE` | `5242880` | Max file size in bytes (5MB). Minimum: 1024 (1KB) |
| `SAS_LOG_WRITE_MAX_FILES` | `5` | Max number of log files (minimum: 1) |

**Example configurations:**

```bash
# Development logging
export SAS_LOG_LEVEL=debug
export SAS_LOG_JSON=false

# Production logging
export SAS_LOG_LEVEL=info
export SAS_LOG_JSON=true
export SAS_LOG_WRITE=true
export SAS_LOG_WRITE_PATH=/var/log/polkadot-rest-api

# Verbose debugging
export SAS_LOG_LEVEL=trace
export SAS_LOG_STRIP_ANSI=true
```

### Migrating from Sidecar

| Sidecar Variable | Status | Notes |
|------------------|--------|-------|
| `SAS_LOG_FILTER_RPC` | Not implemented | polkadot-js API-WS RPC filtering is not applicable (uses subxt, not polkadot-js) |
| `SAS_LOG_LEVEL=silly` | Not supported | Use `trace` instead (most verbose level) |
| `SAS_LOG_LEVEL=verbose` | Not supported | Use `debug` instead |

## Metrics & Monitoring

Enable Prometheus metrics and Loki logging integration.

### Metrics Server

| Variable | Default | Description |
|----------|---------|-------------|
| `SAS_METRICS_ENABLED` | `false` | Enable metrics collection |
| `SAS_METRICS_PROM_HOST` | `127.0.0.1` | Prometheus server host (IP address or hostname) |
| `SAS_METRICS_PROM_PORT` | `9100` | Prometheus server port |
| `SAS_METRICS_PROMETHEUS_PREFIX` | `polkadot_rest_api` | Prometheus metric name prefix |
| `SAS_METRICS_INCLUDE_QUERYPARAMS` | `false` | Include query params in metrics labels |

### Loki Integration

| Variable | Default | Description |
|----------|---------|-------------|
| `SAS_METRICS_LOKI_HOST` | `127.0.0.1` | Loki server host (IP address or hostname) |
| `SAS_METRICS_LOKI_PORT` | `3100` | Loki server port |

### Metrics Endpoints

When `SAS_METRICS_ENABLED=true`, the following endpoints become available:

| Endpoint | Description |
|----------|-------------|
| `GET /metrics` | Prometheus text format |
| `GET /metrics.json` | JSON format |

### Prometheus Prefix

The `SAS_METRICS_PROMETHEUS_PREFIX` must follow Prometheus naming conventions:
- Must start with `[a-zA-Z_:]`
- May contain only `[a-zA-Z0-9_:]`
- Default: `polkadot_rest_api` (all metrics are prefixed, e.g., `polkadot_rest_api_http_requests_total`)

**Example setup:**

```bash
# Enable metrics
export SAS_METRICS_ENABLED=true
export SAS_METRICS_PROM_PORT=9090
export SAS_METRICS_INCLUDE_QUERYPARAMS=true

# With external monitoring stack
export SAS_METRICS_PROM_HOST=prometheus.monitoring.svc
export SAS_METRICS_LOKI_HOST=loki.monitoring.svc

# Custom prefix
export SAS_METRICS_PROMETHEUS_PREFIX=my_api
```

## Environment Profiles

Use different configuration profiles for various environments.

### Using .env Files

Polkadot REST API supports loading configuration from `.env` files using the `--env-file` CLI argument:

```bash
# Use a specific env file
polkadot-rest-api --env-file .env.production

# Default: looks for .env in the current directory
polkadot-rest-api
```

### Example Profiles

**.env.local (Development):**
```bash
SAS_SUBSTRATE_URL=ws://127.0.0.1:9944
SAS_EXPRESS_BIND_HOST=127.0.0.1
SAS_EXPRESS_PORT=8080
SAS_LOG_LEVEL=debug
SAS_METRICS_ENABLED=false
```

**.env.production (Production):**
```bash
SAS_SUBSTRATE_URL=wss://polkadot-rpc.polkadot.io
SAS_EXPRESS_BIND_HOST=0.0.0.0
SAS_EXPRESS_PORT=8080
SAS_LOG_LEVEL=info
SAS_LOG_JSON=true
SAS_LOG_WRITE=true
SAS_LOG_WRITE_PATH=/var/log/polkadot-rest-api
SAS_METRICS_ENABLED=true
SAS_EXPRESS_KEEP_ALIVE_TIMEOUT=30000
```

**.env.docker (Docker):**
```bash
SAS_SUBSTRATE_URL=ws://host.docker.internal:9944
SAS_EXPRESS_BIND_HOST=0.0.0.0
SAS_EXPRESS_PORT=8080
SAS_LOG_JSON=true
```

## Docker Configuration

### Dockerfile

The project uses a multi-stage build:

1. **Build stage**: `rust:1.90.0-slim-bookworm` — compiles the release binary
2. **Runtime stage**: `debian:bookworm-slim` — minimal image with only `ca-certificates`

The final image runs as the `nobody` user and includes these defaults:
- `RUST_LOG=info`
- `SAS_EXPRESS_PORT=8080`
- `SAS_EXPRESS_BIND_HOST=0.0.0.0`

### Running with Docker

```bash
# Pull the image
docker pull docker.io/parity/polkadot-rest-api:latest

# Run with defaults (connects to host node)
docker run --rm -it --read-only -p 8080:8080 \
  -e SAS_SUBSTRATE_URL=ws://host.docker.internal:9944 \
  parity/polkadot-rest-api

# Run with env file
docker run --rm -it --read-only --env-file .env.docker -p 8080:8080 \
  parity/polkadot-rest-api
```

> **Tip**: Always use `--read-only` for containers in production.

### Docker Compose

The `docker-compose.yml` provides a full stack with monitoring:

```yaml
services:
  polkadot:        # Polkadot node
  rest-api:        # Polkadot REST API
  loki:            # Log aggregation
  cadvisor:        # Container metrics
  prometheus:      # Metrics storage
  grafana:         # Dashboards (admin/admin)
```

**Quick start:**
```bash
docker-compose up
```

This starts:
- Polkadot node syncing via warp sync on port `9944`
- REST API on port `8080` (connected to local node)
- Loki on port `3100`
- cAdvisor on port `8081`
- Prometheus on port `9090`
- Grafana on port `3000` (default credentials: `admin`/`admin`)

### Docker Networking Notes

- The Dockerfile sets `SAS_EXPRESS_BIND_HOST=0.0.0.0` by default — no need to override
- Use Docker service names for inter-container communication (e.g., `ws://polkadot:9944`)
- Use `host.docker.internal` to access services on the Docker host

### Migrating from Sidecar Docker Setup

| Change | Details |
|--------|---------|
| **Image name** | `parity/substrate-api-sidecar` → `parity/polkadot-rest-api` |
| **Service name** | `sidecar` → `rest-api` (in docker-compose) |
| **Base image** | Node.js Alpine → Debian slim (smaller runtime, no Node.js needed) |
| **Run user** | `node` → `nobody` |
| **Bind host** | Must set `SAS_EXPRESS_BIND_HOST=0.0.0.0` via env | Set in Dockerfile by default |
| **Health check** | `curl http://localhost:8080/blocks/head` → `curl http://localhost:8080/v1/blocks/head` |
| **Prometheus port** | Separate port `9100` exposed | Served on main port at `/metrics` |

## Configuration Validation

The API validates all configuration on startup and will exit with an error if any value is invalid.

### Testing Configuration

```bash
# Test the API is running
curl http://localhost:8080/v1/blocks/head

# Check health
curl http://localhost:8080/v1/health

# Check metrics (if enabled)
curl http://localhost:8080/metrics

# Verify node connection
curl http://localhost:8080/v1/node/version

# Check API version
curl http://localhost:8080/v1/version

# Check capabilities (supported pallets, chain type)
curl http://localhost:8080/v1/capabilities
```

### Common Issues

**Connection refused:**
- Check `SAS_SUBSTRATE_URL` is reachable
- Verify WebSocket/HTTP protocol matches the node

**Docker networking:**
- The Dockerfile sets `SAS_EXPRESS_BIND_HOST=0.0.0.0` by default
- Check port mapping: `-p host_port:container_port`

**Invalid configuration:**
- All env vars are validated on startup — check the error message
- `SAS_EXPRESS_PORT` must be non-zero
- `SAS_EXPRESS_REQUEST_LIMIT` must be non-zero
- `SAS_LOG_LEVEL` must be one of: `trace`, `debug`, `http`, `info`, `warn`, `error`
- `SAS_METRICS_PROMETHEUS_PREFIX` must follow Prometheus naming: start with `[a-zA-Z_:]`, contain only `[a-zA-Z0-9_:]`

**Performance tuning:**
- Increase `SAS_EXPRESS_BLOCK_FETCH_CONCURRENCY` for faster block range queries (default: 10)
- Tune `SAS_EXPRESS_KEEP_ALIVE_TIMEOUT` for long-running connections (default: 5000ms)
- Adjust `SAS_SUBSTRATE_RECONNECT_*` values for unreliable RPC connections

## Complete Example

Full production configuration:

```bash
#!/bin/bash
# Production Polkadot REST API Configuration

# Server
export SAS_EXPRESS_BIND_HOST=0.0.0.0
export SAS_EXPRESS_PORT=8080
export SAS_EXPRESS_KEEP_ALIVE_TIMEOUT=30000
export SAS_EXPRESS_REQUEST_LIMIT=512000
export SAS_EXPRESS_BLOCK_FETCH_CONCURRENCY=20

# Blockchain connection
export SAS_SUBSTRATE_URL=wss://polkadot-rpc.polkadot.io

# Reconnection
export SAS_SUBSTRATE_RECONNECT_INITIAL_DELAY_MS=100
export SAS_SUBSTRATE_RECONNECT_MAX_DELAY_MS=10000
export SAS_SUBSTRATE_RECONNECT_REQUEST_TIMEOUT_MS=30000

# Asset Hub multi-chain setup
export SAS_SUBSTRATE_MULTI_CHAIN_URL='[
  {"url":"wss://polkadot-rpc.polkadot.io","type":"relay"}
]'

# Logging
export SAS_LOG_LEVEL=info
export SAS_LOG_JSON=true
export SAS_LOG_WRITE=true
export SAS_LOG_WRITE_PATH=/var/log/polkadot-rest-api
export SAS_LOG_WRITE_MAX_FILE_SIZE=10485760  # 10MB
export SAS_LOG_WRITE_MAX_FILES=10

# Metrics
export SAS_METRICS_ENABLED=true
export SAS_METRICS_PROM_HOST=0.0.0.0
export SAS_METRICS_PROM_PORT=9100
export SAS_METRICS_PROMETHEUS_PREFIX=polkadot_rest_api
export SAS_METRICS_LOKI_HOST=loki.monitoring.svc
export SAS_METRICS_LOKI_PORT=3100

# Start
polkadot-rest-api
```

---

For migration details from Substrate API Sidecar, see the [Migration Guide](./MIGRATION.md).
