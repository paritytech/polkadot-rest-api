# Integration Tests

This crate contains integration tests for the Polkadot REST API.

## Project Structure

```
crates/integration_tests/
├── src/
│   ├── lib.rs              # Library exports
│   ├── client.rs           # HTTP test client
│   ├── config.rs           # Test configuration loader
│   ├── fixtures.rs         # Fixture loading utilities
│   ├── test_helpers.rs     # Common test helpers and macros
│   └── utils.rs            # JSON comparison utilities
├── tests/
│   ├── accounts/           # Account endpoint tests (12 submodules)
│   ├── coretime/           # Coretime endpoint tests (6 submodules)
│   │   ├── mod.rs
│   │   ├── info.rs
│   │   ├── leases.rs
│   │   ├── overview.rs
│   │   ├── regions.rs
│   │   ├── renewals.rs
│   │   └── reservations.rs
│   ├── basic.rs            # Health/version endpoint tests
│   ├── capabilities.rs     # Capabilities endpoint tests
│   ├── chain_config.rs     # Chain configuration tests
│   ├── historical.rs       # Fixture-based regression tests
│   ├── latest.rs           # Latest block endpoint tests
│   ├── relay_chain_connection.rs
│   ├── use_rc_block.rs     # RC block parameter tests
│   ├── config/
│   │   └── test_config.json
│   └── fixtures/           # Pre-recorded JSON responses
│       ├── polkadot/
│       │   ├── blocks/
│       │   ├── pallets/{pallet_name}/
│       │   ├── runtime/
│       │   ├── coretime/
│       │   ├── paras/
│       │   └── staking/
│       ├── kusama/
│       ├── asset-hub-polkadot/
│       ├── asset-hub-kusama/
│       ├── coretime-polkadot/
│       ├── coretime-kusama/
│       └── common/
└── scripts/
    ├── migrate_fixtures.sh      # Migrate flat fixtures to nested structure
    └── update_fixture_paths.py  # Update paths in test_config.json
```

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

### 4. Coretime Tests (`tests/coretime/`)

Comprehensive tests for coretime endpoints, organized by endpoint:

- `leases.rs` - /v1/coretime/leases (10 tests)
- `reservations.rs` - /v1/coretime/reservations (8 tests)
- `renewals.rs` - /v1/coretime/renewals (10 tests)
- `regions.rs` - /v1/coretime/regions (10 tests)
- `info.rs` - /v1/coretime/info (10 tests)
- `overview.rs` - /v1/coretime/overview (13 tests)

### 5. Account Tests (`tests/accounts/`)

Tests for account-related endpoints with 12 submodules covering balance info, staking, vesting, proxies, and assets.

### 6. Use RC Block Tests (`tests/use_rc_block.rs`)

Tests for the `useRcBlock` query parameter feature, which allows querying parachain data via relay chain blocks.

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

# For Coretime chains
export SAS_SUBSTRATE_URL=wss://kusama-coretime-rpc.polkadot.io
cargo run --release --bin polkadot-rest-api
```

### Run Commands

```bash
# Run all integration tests
cargo test --package integration_tests

# Run specific test suites
cargo test --package integration_tests --test historical
cargo test --package integration_tests --test latest
cargo test --package integration_tests --test basic

# Run coretime tests (all submodules)
cargo test --package integration_tests coretime

# Run specific coretime submodule
cargo test --package integration_tests coretime::leases
cargo test --package integration_tests coretime::overview

# Run a specific chain's historical tests
cargo test --package integration_tests --test historical test_historical_polkadot

# View detailed output
cargo test --package integration_tests --test historical -- --nocapture
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

## Test Helpers

The `src/test_helpers.rs` module provides common utilities to reduce boilerplate:

### Setup Helpers

```rust
use integration_tests::test_helpers::{init_tracing, setup_client};

#[tokio::test]
async fn my_test() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;
    // ...
}
```

### Chain Detection

```rust
use integration_tests::test_helpers::{is_coretime_chain, is_relay_chain, has_pallet};

// Check if connected to a coretime chain
if !is_coretime_chain(&client).await {
    return skip_test("Not a coretime chain");
}

// Check for specific pallet
if !has_pallet(&client, "Staking").await {
    return skip_test("Staking pallet not found");
}
```

### Assertion Helpers

```rust
use integration_tests::test_helpers::{
    assert_valid_at_field,
    assert_array_field,
    assert_string_field,
    assert_bad_request,
};

// Validate response structure
assert_valid_at_field(&json)?;
let items = assert_array_field(&json, "leases")?;
let hash = assert_string_field(&json, "hash")?;

// Test error responses
assert_bad_request(&client, "/v1/endpoint?at=invalid").await?;
```

### Macros

```rust
use integration_tests::{require_coretime_chain, require_pallet, skip_if};

#[tokio::test]
async fn test_coretime_feature() -> Result<()> {
    let client = setup_client().await?;

    // Skip if not on coretime chain
    require_coretime_chain!(&client);

    // Skip if missing specific pallet
    require_pallet!(&client, "Broker");

    // Custom skip condition
    skip_if!(some_condition, "Reason to skip");

    // ... test code
}
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
        "fixture_path": "polkadot/blocks/1000000.json"
      }
    ]
  }
}
```

### Fixtures

Pre-recorded JSON responses organized by chain and feature:

```
tests/fixtures/
├── polkadot/
│   ├── blocks/
│   │   ├── 1000000.json
│   │   └── range_1000000-1000002.json
│   ├── pallets/
│   │   ├── balances/
│   │   │   ├── errors.json
│   │   │   └── dispatchables_28500000.json
│   │   └── system/
│   │       └── storage_20000000.json
│   ├── runtime/
│   │   ├── metadata_21000000.json
│   │   └── spec_28500000.json
│   └── coretime/
│       └── overview_24000000.json
├── kusama/
├── asset-hub-polkadot/
└── ...
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
  "fixture_path": "polkadot/runtime/spec_1000000.json",
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

### Adding a New Test Module

For a new feature area, create a module directory:

```bash
mkdir tests/new_feature
```

Create `tests/new_feature/mod.rs`:

```rust
// Re-export common helpers
pub use integration_tests::test_helpers::{init_tracing, setup_client};

mod endpoint_a;
mod endpoint_b;
```

Create individual test files (e.g., `tests/new_feature/endpoint_a.rs`):

```rust
use super::{init_tracing, setup_client};
use anyhow::Result;

#[tokio::test]
async fn test_endpoint_a_structure() -> Result<()> {
    init_tracing();
    let client = setup_client().await?;

    let (status, json) = client.get_json("/v1/new-feature/endpoint-a").await?;
    assert!(status.is_success());

    Ok(())
}
```

## Updating Fixtures

When API responses change intentionally, update fixtures:

```bash
# Update all fixtures
cargo run --bin update_fixtures

# Update fixtures for a specific chain
cargo run --bin update_fixtures -- polkadot
cargo run --bin update_fixtures -- kusama
```

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `API_URL` | `http://localhost:8080` | Base URL of the API server |
| `TEST_CONFIG_PATH` | `tests/config/test_config.json` | Path to test configuration |
| `FIXTURES_DIR` | `tests/fixtures` | Path to fixture files |
| `RUST_LOG` | - | Log level (e.g., `info`, `debug`) |
| `TEST_DELAY_MS` | `0` | Delay between requests (rate limiting) |

## JSON Comparison

The historical tests use a sophisticated JSON comparison system that:

- Recursively compares nested objects and arrays
- Ignores specified fields (e.g., `timestamp`, `at`)
- Reports detailed diffs with colored output
- Handles extra/missing fields gracefully

Ignored fields in historical tests:
- `timestamp`
- `authorId`
