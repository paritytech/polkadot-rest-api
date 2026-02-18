# Polkadot REST API

> **Note:** This project is in a beta release. Changes are to be expected until it's stable. 

## Implementation Details

### Logging

Logging levels supported are ```trace, debug, info, http, warn, error```. **http** level allows for the emission of http information logging (method, route, elapsed time, success code). However currently tracing does not support *http*.  To mitigate this, **http** level falls back to *debug* for successful logs, *warn* for 4** request logs, and *error* for 5**

## Metrics and Monitoring

The API exposes Prometheus metrics at `/metrics`. To enable metrics collection, set:

```bash
export SAS_METRICS_ENABLED=true
```

A sample Grafana dashboard is provided in `metrics/grafana/provisioning/dashboards/` for visualizing metrics.

### docker compose

When running locally with `docker compose`, the Grafana dashboard is accessible at http://localhost:3000/d/polkadot-rest-api

If needed, the login and password for grafana are set to "admin" and "admin" respectfully.

All container resources are shown despite only the `rest-api` container being useful.
To map the short ids for a container name you can run

```bash
docker ps --format '{{.ID}}: {{.Names}}'
```

Prometheus is accessible at http://localhost:9090/

Loki logs can be viewed in Grafana at Explore > Loki (select Loki as the datasource and query with `{service_name="polkadot-rest-api"}`)

## Benchmarks

### Benchmark Workflows

The repository includes two main benchmark workflows that run automatically on pushes and pull requests to the `main` branch:

#### 1. Benchmark workflow

- Builds and starts the server
- Runs performance tests against all configured endpoints
- Measures throughput (requests/second) and latency metrics (P50, P90, P99)
- Publishes results to GitHub Pages for historical tracking

**GitHub Pages Dashboard**: [https://paritytech.github.io/polkadot-rest-api/dev/bench/](https://paritytech.github.io/polkadot-rest-api/dev/bench/)

#### 2. Benchmark Comparison (vs Sidecar)


- Runs benchmarks against public Sidecar instance
- Calculates performance differences and improvements
- Generates comparison reports with percentage differences
- Publishes comparison metrics to GitHub Pages for trend analysis

**GitHub Pages Dashboard**: [https://paritytech.github.io/polkadot-rest-api/dev/bench/comparison/](https://paritytech.github.io/polkadot-rest-api/dev/bench/comparison/)

### Benchmark Metrics

Both workflows track the following metrics:

- **Throughput**: Requests per second (higher is better)
- **Average Latency**: Mean response time in milliseconds (lower is better)
- **P50 Latency**: 50th percentile latency (lower is better)
- **P90 Latency**: 90th percentile latency (lower is better)
- **P99 Latency**: 99th percentile latency (lower is better)

## Testing

### Unit Tests

Unit tests are embedded in the source code and test individual functions and modules.

**Run all unit tests:**
```bash
cargo test --workspace --all-features
```

### Integration Tests

These tests are located in `crates/integration_tests/tests/`.

#### Available Integration Test Suites

1. **basic.rs** - Tests basic API endpoints (health, version)
2. **latest.rs** - Tests endpoints with the latest blockchain data
3. **historical.rs** - Tests endpoints with historical blockchain data

#### Test Configuration

Test definitions are located in `crates/integration_tests/tests/config/test_config.json`. To add new integration tests, add them to this configuration file.

#### Running Integration Tests

**Step 1:** Start the API server in one terminal

For Polkadot:
```bash
export SAS_SUBSTRATE_URL=wss://rpc.polkadot.io
cargo run --release --bin polkadot-rest-api
```

For Kusama:
```bash
export SAS_SUBSTRATE_URL=wss://kusama-rpc.polkadot.io
cargo run --release --bin polkadot-rest-api
```

**Step 2:** Run tests in another terminal

```bash
# Run all integration tests
cargo test --package integration_tests

# Run a specific test suite (recommended - cleaner output)
cargo test --package integration_tests --test historical  # All historical tests
cargo test --package integration_tests --test latest      # All latest tests
cargo test --package integration_tests --test basic       # All basic tests
```

**Running tests for a specific chain:**

```bash
# Historical tests (use fixtures for regression testing)
cargo test --package integration_tests --test historical test_historical_polkadot
cargo test --package integration_tests --test historical test_historical_kusama
cargo test --package integration_tests --test historical test_historical_asset_hub_polkadot
cargo test --package integration_tests --test historical test_historical_asset_hub_kusama

# Latest tests (test against live blockchain data)
cargo test --package integration_tests --test latest test_latest_polkadot
cargo test --package integration_tests --test latest test_latest_kusama
cargo test --package integration_tests --test latest test_latest_asset_hub_polkadot
cargo test --package integration_tests --test latest test_latest_asset_hub_kusama
```

**Viewing test output:**

By default, cargo captures test output. To see the detailed test progress with checkmarks and colored output, add `-- --nocapture`:

```bash
cargo test --package integration_tests --test historical test_historical_polkadot -- --nocapture
```

Example output:
```
Running 2 historical test cases for chain: polkadot

✓ /v1/blocks/{blockId} (block 1000000)
✓ /v1/blocks/{blockId} (block 10000000)

════════════════════════════════════════════════════════════
Historical Test Results for polkadot
════════════════════════════════════════════════════════════
  ✓ Passed: 2
  ✗ Failed: 0
════════════════════════════════════════════════════════════
```

### Updating Test Fixtures

To update test fixtures with current blockchain data:

```bash
./scripts/update_fixtures.sh
```

### Using `.env` files

The application supports configuration via `.env` files (for example, `.env` or `.env.polkadot`), allowing you to define all environment variables in one place. A sample configuration is available in [.env.sample](./.env.sample).
To start `polkadot-rest-api` with a specific `.env` file, pass the file path as a command-line argument:
```bash
cargo run --release --bin polkadot-rest-api -- --env-file .env.polkadot
```
This command loads the environment variables specified in `.env.polkadot` file, which should be located in the project's root directory.

### Multi-Chain Configuration

For Asset Hub deployments that need relay chain access (e.g., for `useRcBlock` parameter support or `/rc/` related endpoints), configure `SAS_SUBSTRATE_MULTI_CHAIN_URL`:

```bash
# Primary connection: Asset Hub
export SAS_SUBSTRATE_URL=wss://polkadot-asset-hub-rpc.polkadot.io

# Additional chain: Relay chain (enables useRcBlock)
export SAS_SUBSTRATE_MULTI_CHAIN_URL='[{"url":"wss://rpc.polkadot.io","type":"relay"}]'
```

**Supported chain types:**
- `relay` - Relay chain (Polkadot, Kusama, Westend, etc.)
- `assethub` - Asset Hub parachain
- `parachain` - Generic parachain
