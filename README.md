# Polkadot REST API

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

# Run basic endpoint tests (chain-agnostic)
cargo test --package integration_tests --test basic

### Updating Test Fixtures

To update test fixtures with current blockchain data:

```bash
./scripts/update_fixtures.sh
```
