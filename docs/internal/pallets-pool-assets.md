# Pallets Pool Assets Endpoint

> **Internal Document** - Implementation spec for pool-assets endpoint

## Endpoint

### `GET /v1/pallets/pool-assets/{assetId}/asset-info`

Returns information about a specific pool asset (LP token) on Asset Hub chains.

---

## API Specification

### Path Parameters
| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `assetId` | string | Yes | Pool asset ID (unsigned integer) |

### Query Parameters
| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `at` | string | latest | Block height or hash |
| `useRcBlock` | boolean | false | Use relay chain block (Asset Hub only) |

### Response: `PalletPoolAssetsInfoResponse`
```json
{
  "at": {
    "hash": "0xabc123...",
    "height": "10260000"
  },
  "poolAssetInfo": {
    "owner": "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
    "issuer": "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
    "admin": "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
    "freezer": "5GrwvaEF5zXb26Fz9rcQpDWS57CtERHpNehXCPcNoHGKutQY",
    "supply": "1000000000000",
    "deposit": "0",
    "minBalance": "1",
    "isSufficient": false,
    "accounts": "10",
    "sufficients": "0",
    "approvals": "0",
    "status": "Live"
  },
  "poolAssetMetadata": {
    "deposit": "0",
    "name": "LP Token DOT-USDT",
    "symbol": "LP-DOT-USDT",
    "decimals": "12",
    "isFrozen": false
  },
  "rcBlockHash": "0x...",      // only with useRcBlock
  "rcBlockNumber": "...",      // only with useRcBlock
  "ahTimestamp": "..."         // only with useRcBlock
}
```

---

## Implementation Details

### File Structure
```
crates/server/src/handlers/pallets/
├── pool_assets.rs   (NEW)
├── assets.rs        (reference - nearly identical)
├── common.rs        (shared types - reuse AssetInfo, AssetMetadata)
├── mod.rs           (add export)
└── ...
```

### Relationship to Assets Endpoint

This endpoint is **nearly identical** to `/pallets/assets/{assetId}/asset-info`:

| Aspect | Assets | Pool Assets |
|--------|--------|-------------|
| Storage Pallet | `Assets` | `PoolAssets` |
| Storage Items | `Asset`, `Metadata` | `Asset`, `Metadata` |
| Response Structure | Same | Same |
| Chains | Asset Hub only | Asset Hub only |

### Key Difference
- Query `PoolAssets::Asset` instead of `Assets::Asset`
- Query `PoolAssets::Metadata` instead of `Assets::Metadata`

### Response Types

Can **reuse** types from `assets.rs` or `common.rs`:

```rust
/// Response for `/pallets/pool-assets/{assetId}/asset-info`
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PalletPoolAssetsInfoResponse {
    pub at: AtResponse,
    pub pool_asset_info: Option<AssetInfo>,
    pub pool_asset_metadata: Option<AssetMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}

// Reuse from common.rs or assets.rs
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetInfo {
    pub owner: String,
    pub issuer: String,
    pub admin: String,
    pub freezer: String,
    pub supply: String,
    pub deposit: String,
    pub min_balance: String,
    pub is_sufficient: bool,
    pub accounts: String,
    pub sufficients: String,
    pub approvals: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetMetadata {
    pub deposit: String,
    pub name: String,
    pub symbol: String,
    pub decimals: String,
    pub is_frozen: bool,
}
```

### Storage Queries

```rust
// Storage key prefix for PoolAssets::Asset
// twox_128("PoolAssets") ++ twox_128("Asset") ++ blake2_128_concat(asset_id)
let pool_asset_prefix = "0x..."; // Calculate or hardcode

// Query PoolAssets::Asset(asset_id)
let asset_key = format!(
    "{}{}",
    pool_asset_prefix,
    encode_blake2_128_concat(asset_id)
);

// Query PoolAssets::Metadata(asset_id)
let metadata_prefix = "0x..."; // Calculate or hardcode
let metadata_key = format!(
    "{}{}",
    metadata_prefix,
    encode_blake2_128_concat(asset_id)
);
```

### SCALE Decoding

Same structure as regular assets:

```rust
#[derive(Debug, Clone, Decode)]
pub struct AssetDetails {
    pub owner: [u8; 32],
    pub issuer: [u8; 32],
    pub admin: [u8; 32],
    pub freezer: [u8; 32],
    pub supply: u128,
    pub deposit: u128,
    pub min_balance: u128,
    pub is_sufficient: bool,
    pub accounts: u32,
    pub sufficients: u32,
    pub approvals: u32,
    pub status: AssetStatus,
}

#[derive(Debug, Clone, Decode)]
pub enum AssetStatus {
    Live,
    Frozen,
    Destroying,
}

#[derive(Debug, Clone, Decode)]
pub struct AssetMetadataStorage {
    pub deposit: u128,
    pub name: Vec<u8>,    // BoundedVec
    pub symbol: Vec<u8>,  // BoundedVec
    pub decimals: u8,
    pub is_frozen: bool,
}
```

---

## Sidecar Reference

### Service: Similar to `PalletsAssetsService.ts`

Sidecar handles both `Assets` and `PoolAssets` with the same logic, just different pallet names.

---

## Test Fixtures Needed

Pool assets are only available on **Asset Hub chains**:

### Asset Hub Polkadot (block 10,260,000)
- `pallets_pool_assets_0_asset_info_10260000.json` - Pool asset 0 info

### Asset Hub Kusama (block 11,152,000)
- `pallets_pool_assets_0_asset_info_11152000.json` - Pool asset 0 info

**Note**: Need to find valid pool asset IDs that exist at these blocks. May need to query storage to find existing LP tokens.

### Finding Valid Pool Asset IDs

```bash
# Query all pool assets at a block
curl "http://localhost:8082/v1/pallets/PoolAssets/storage/Asset?at=10260000"
```

Or check `AssetConversion` liquidity pools to find `lpToken` values.

---

## Routes to Add

```rust
// In routes/pallets.rs
.route_registered(
    registry,
    API_VERSION,
    "/pallets/pool-assets/:asset_id/asset-info",
    "get",
    get(pallets::pallets_pool_assets_info),
)
```

---

## Chain Availability

| Chain | Available |
|-------|-----------|
| Polkadot | ❌ No |
| Kusama | ❌ No |
| Asset Hub Polkadot | ✅ Yes |
| Asset Hub Kusama | ✅ Yes |

Should return appropriate error on non-Asset Hub chains.

---

## Implementation Strategy

### Option A: Copy and Modify `assets.rs`
- Quick to implement
- Some code duplication

### Option B: Refactor to Share Code
- Create generic function for both Assets and PoolAssets
- Pass pallet name as parameter
- Cleaner but more refactoring

**Recommendation**: Option A for speed, can refactor later.

---

## Checklist

- [ ] Create `pool_assets.rs` handler
- [ ] Reuse AssetInfo/AssetMetadata types from assets.rs
- [ ] Implement storage query for PoolAssets pallet
- [ ] Add `useRcBlock` support
- [ ] Register route
- [ ] Find valid pool asset IDs at test blocks
- [ ] Capture fixtures from Sidecar
- [ ] Add test config entries
- [ ] Add unit tests
- [ ] Run fmt/clippy
- [ ] Test on Asset Hub chains

---

## Testing Configuration

### Sidecar Ports (Historical Data)
| Chain | Port |
|-------|------|
| Asset Hub Polkadot | 8072 |
| Asset Hub Kusama | 8073 |

### PAPI Ports (polkadot-rest-api)
| Chain | Port |
|-------|------|
| Asset Hub Polkadot | 8082 |
| Asset Hub Kusama | 8083 |

### Capturing Fixtures from Sidecar

```bash
# First, find valid pool asset IDs by checking PoolAssets storage
# Asset Hub Polkadot
curl "http://localhost:8072/pallets/PoolAssets/storage/Asset?at=10260000" | jq .

# Asset Hub Kusama
curl "http://localhost:8073/pallets/PoolAssets/storage/Asset?at=11152000" | jq .

# Once you find a valid pool asset ID (e.g., 0 or another LP token ID):

# Asset Hub Polkadot - Pool Asset info
curl "http://localhost:8072/pallets/pool-assets/0/asset-info?at=10260000" | jq . > tests/fixtures/asset-hub-polkadot/pallets_pool_assets_0_asset_info_10260000.json

# Asset Hub Kusama - Pool Asset info
curl "http://localhost:8073/pallets/pool-assets/0/asset-info?at=11152000" | jq . > tests/fixtures/asset-hub-kusama/pallets_pool_assets_0_asset_info_11152000.json
```

### Running Tests

Run tests for a specific chain with `API_URL` environment variable:

```bash
# Test against Asset Hub Polkadot PAPI instance on port 8082
API_URL=http://localhost:8082 cargo test --package integration_tests --test historical test_historical_asset_hub_polkadot -- --nocapture

# Test against Asset Hub Kusama PAPI instance on port 8083
API_URL=http://localhost:8083 cargo test --package integration_tests --test historical test_historical_asset_hub_kusama -- --nocapture
```

### Test Config Entries to Add

Add these to `test_config.json` under `historical_tests`:

```json
// In "asset-hub-polkadot" array:
{
  "endpoint": "/v1/pallets/pool-assets/{assetId}/asset-info",
  "asset_id": "0",
  "query_params": { "at": "10260000" },
  "fixture_path": "asset-hub-polkadot/pallets_pool_assets_0_asset_info_10260000.json",
  "description": "Test pool assets asset-info for pool asset 0 at Asset Hub Polkadot block 10,260,000"
}

// In "asset-hub-kusama" array:
{
  "endpoint": "/v1/pallets/pool-assets/{assetId}/asset-info",
  "asset_id": "0",
  "query_params": { "at": "11152000" },
  "fixture_path": "asset-hub-kusama/pallets_pool_assets_0_asset_info_11152000.json",
  "description": "Test pool assets asset-info for pool asset 0 at Asset Hub Kusama block 11,152,000"
}
```
