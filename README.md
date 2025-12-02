# Polkadot REST API

## Implementation Details

### Logging

Logging levels supported are ```race, debug, info, http, warn, error```. **http** level allows for the emission of http information logging (method, route, elapsed time, success code). However currently tracing does not support *http*.  To mitigate this, **http** level falls back to *debug* for successful logs, *warn* for 4** request logs, and *error* for 5**

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

# Run tests for a specific chain
cargo test --package integration_tests test_latest_polkadot
cargo test --package integration_tests test_latest_kusama
cargo test --package integration_tests test_historical_polkadot
cargo test --package integration_tests test_historical_kusama
cargo test --package integration_tests test_historical_asset_hub_polkadot
cargo test --package integration_tests test_historical_asset_hub_kusama

# Run basic endpoint tests (chain-agnostic)
cargo test --package integration_tests --test basic

### Updating Test Fixtures

To update test fixtures with current blockchain data:

```bash
./scripts/update_fixtures.sh
```
