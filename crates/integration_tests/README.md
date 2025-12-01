# Integration Tests

This crate contains integration tests for the Polkadot REST API.

## Test Suites

### 1. Basic Tests (`tests/basic.rs`)

Smoke tests for fundamental API endpoints. These are chain-agnostic and verify basic functionality.

- Health endpoint validation
- Version endpoint validation
- Error handling for invalid endpoints
- Concurrent request handling

### 2. Latest Tests (`tests/latest.rs`)

Tests that run against the current/latest blockchain data. These verify that endpoints work correctly with live data.

- Tests configured endpoints from `test_config.json`
- Fetches the latest block number dynamically
- Validates HTTP status codes

### 3. Historical Tests (`tests/historical.rs`)

Regression tests using pre-recorded fixtures. These ensure API responses remain consistent across code changes.

- Compares API responses against stored JSON fixtures
- Tests specific historical blocks (e.g., block 1,000,000)
- Ignores time-varying fields (timestamps, etc.)

## Running Tests

### Prerequisites

Start the API server connected to the appropriate chain:

```bash
# For Polkadot
export SAS_SUBSTRATE_URL=wss://rpc.polkadot.io
cargo run --release --bin polkadot-rest-api

# For Kusama
export SAS_SUBSTRATE_URL=wss://kusama-rpc.polkadot.io
cargo run --release --bin polkadot-rest-api
```

### Run Commands

```bash
# Run all tests in a suite (recommended)
cargo test --package integration_tests --test historical
cargo test --package integration_tests --test latest
cargo test --package integration_tests --test basic

# Run a specific chain's tests
cargo test --package integration_tests --test historical test_historical_polkadot
cargo test --package integration_tests --test latest test_latest_polkadot

# View detailed output with progress indicators
cargo test --package integration_tests --test historical test_historical_polkadot -- --nocapture
```

### Understanding the Output

With `--nocapture`, tests display progress as they run:

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

## Configuration

### Test Configuration (`tests/config/test_config.json`)

Defines chains, endpoints, and test cases:

```json
{
  "chains": [
    { "name": "polkadot", "api_url": "http://localhost:8080" }
  ],
  "latest_endpoints": [
    { "path": "/v1/health", "query_params": [{}] }
  ],
  "historical_tests": {
    "polkadot": [
      {
        "endpoint": "/v1/blocks/{blockId}",
        "block_height": 1000000,
        "fixture_path": "polkadot/blocks_1000000.json"
      }
    ]
  }
}
```

### Fixtures (`tests/fixtures/`)

Pre-recorded JSON responses organized by chain:

```
tests/fixtures/
├── polkadot/
│   ├── blocks_1000000.json
│   └── blocks_10000000.json
├── kusama/
│   └── blocks_5000000.json
├── asset-hub-polkadot/
│   ├── blocks_10250000_pre_migration.json
│   └── blocks_10260000_post_migration.json
└── asset-hub-kusama/
    ├── blocks_11150000_pre_migration.json
    └── blocks_11152000_post_migration.json
```

## Adding New Tests

### Adding a Latest Endpoint Test

1. Add the endpoint to `latest_endpoints` in `test_config.json`:

```json
{
  "path": "/v1/runtime/metadata",
  "query_params": [{}],
  "requires_block_height": false,
  "requires_account": false
}
```

### Adding a Historical Test

1. Add the test case to `historical_tests` in `test_config.json`:

```json
{
  "endpoint": "/v1/runtime/spec",
  "block_height": 1000000,
  "fixture_path": "polkadot/runtime_spec_1000000.json",
  "description": "Test runtime spec at block 1,000,000"
}
```

2. Generate the fixture:

```bash
# Start server connected to the chain
cargo run --release --bin polkadot-rest-api

# Run the fixture update tool
cargo run --bin update_fixtures -- polkadot
```

Or manually create the fixture by calling the endpoint and saving the response.

## Updating Fixtures

When API responses change intentionally, update fixtures:

```bash
# Update all fixtures
cargo run --bin update_fixtures

# Update fixtures for a specific chain
cargo run --bin update_fixtures -- polkadot
cargo run --bin update_fixtures -- kusama
```

Or use the convenience script:

```bash
./scripts/update_fixtures.sh
```

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `API_URL` | `http://localhost:8080` | Base URL of the API server |
| `TEST_CONFIG_PATH` | `tests/config/test_config.json` | Path to test configuration |
| `FIXTURES_DIR` | `tests/fixtures` | Path to fixture files |
| `RUST_LOG` | - | Log level (e.g., `info`, `debug`) |

## JSON Comparison

The historical tests use a sophisticated JSON comparison system that:

- Recursively compares nested objects and arrays
- Ignores specified fields (e.g., `timestamp`, `at`)
- Reports detailed diffs with colored output
- Handles extra/missing fields gracefully

Ignored fields in historical tests:
- `timestamp`
- `at`
- `blockNumber`
- `blockHash`
