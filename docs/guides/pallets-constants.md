# Pallets Constants Endpoints

> **Internal Document** - Implementation spec for constants endpoints

## Endpoints

### 1. `GET /v1/pallets/{palletId}/consts`

Returns all constants defined in a pallet.

### 2. `GET /v1/pallets/{palletId}/consts/{constantItemId}`

Returns a specific constant by name.

---

## API Specification

### List Endpoint: `/pallets/{palletId}/consts`

#### Path Parameters
| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `palletId` | string | Yes | Pallet name (case-insensitive) or index |

#### Query Parameters
| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `at` | string | latest | Block height or hash |
| `onlyIds` | boolean | false | Return only constant names |
| `useRcBlock` | boolean | false | Use relay chain block (Asset Hub only) |

#### Response: `PalletConstantsResponse`
```json
{
  "at": {
    "hash": "0x...",
    "height": "28490503"
  },
  "pallet": "balances",
  "palletIndex": "10",
  "items": [
    {
      "name": "ExistentialDeposit",
      "type": "6",
      "value": "10000000000",
      "docs": [
        "The minimum amount required to keep an account open.",
        "MUST BE GREATER THAN ZERO!"
      ]
    }
  ],
  "rcBlockHash": "0x...",      // only with useRcBlock
  "rcBlockNumber": "...",      // only with useRcBlock
  "ahTimestamp": "..."         // only with useRcBlock
}
```

#### Response with `onlyIds=true`
```json
{
  "at": { "hash": "0x...", "height": "28490503" },
  "pallet": "balances",
  "palletIndex": "10",
  "items": ["ExistentialDeposit", "MaxLocks", "MaxReserves", "MaxFreezes"]
}
```

---

### Item Endpoint: `/pallets/{palletId}/consts/{constantItemId}`

#### Path Parameters
| Parameter | Type | Required | Description |
|-----------|------|----------|-------------|
| `palletId` | string | Yes | Pallet name or index |
| `constantItemId` | string | Yes | Constant name (case-insensitive) |

#### Query Parameters
| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `at` | string | latest | Block height or hash |
| `metadata` | boolean | false | Include full metadata |
| `useRcBlock` | boolean | false | Use relay chain block (Asset Hub only) |

#### Response: `PalletConstantItemResponse`
```json
{
  "at": {
    "hash": "0x...",
    "height": "28490503"
  },
  "pallet": "balances",
  "palletIndex": "10",
  "constantItem": "existentialDeposit",
  "value": "10000000000",
  "metadata": {                    // only with metadata=true
    "name": "ExistentialDeposit",
    "type": "6",
    "docs": ["The minimum amount required..."]
  }
}
```

---

## Implementation Details

### File Structure
```
crates/server/src/handlers/pallets/
├── constants.rs     (NEW)
├── common.rs        (shared types)
├── mod.rs           (add export)
└── ...
```

### Response Types

```rust
/// Response for `/pallets/{palletId}/consts`
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PalletConstantsResponse {
    pub at: AtResponse,
    pub pallet: String,
    pub pallet_index: String,
    pub items: ConstantsItems,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum ConstantsItems {
    Full(Vec<ConstantItemMetadata>),
    OnlyIds(Vec<String>),
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConstantItemMetadata {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub value: String,
    pub docs: Vec<String>,
}

/// Response for `/pallets/{palletId}/consts/{constantItemId}`
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PalletConstantItemResponse {
    pub at: AtResponse,
    pub pallet: String,
    pub pallet_index: String,
    pub constant_item: String,
    pub value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<ConstantItemMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rc_block_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ah_timestamp: Option<String>,
}
```

### Metadata Extraction

#### V14/V15/V16 (Modern)
```rust
// pallet.constants is Vec<PalletConstantMetadata>
for constant in &pallet.constants {
    let name = constant.name.clone();
    let ty = constant.ty.id.to_string();
    let value = hex::encode(&constant.value);  // or decode using type registry
    let docs = constant.docs.clone();
}
```

#### V9-V13 (Legacy)
```rust
// pallet.constants is DecodeDifferent<_, Vec<ModuleConstantMetadata>>
if let DecodeDifferent::Decoded(constants) = &pallet.constants {
    for constant in constants {
        let name = decode_different_str(&constant.name);
        let ty = decode_different_str(&constant.ty);
        let value = hex::encode(&constant.value);
        let docs = decode_different_vec(&constant.documentation);
    }
}
```

### Value Decoding

The `value` field in constants is SCALE-encoded bytes. Options:

1. **Hex string** (simplest, what Sidecar does):
   ```rust
   let value = format!("0x{}", hex::encode(&constant.value));
   ```

2. **Decoded JSON** (more complex):
   - Use type registry to decode
   - Convert to JSON representation
   - More useful but more complex

**Recommendation**: Start with hex encoding to match Sidecar, can enhance later.

---

## Sidecar Reference

### Service: `PalletsConstantsService.ts`

Key methods:
- `fetchConstantsMeta()` - Get all constants metadata
- `fetchConstantItemMeta()` - Get single constant

### Response format from Sidecar:
```json
{
  "at": { "hash": "0x...", "height": "28490503" },
  "pallet": "balances",
  "palletIndex": "10",
  "items": [
    {
      "name": "ExistentialDeposit",
      "type": "6",
      "value": "0x00e40b5402000000000000000000000000",
      "docs": ["The minimum amount required..."]
    }
  ]
}
```

---

## Test Fixtures Needed

### Polkadot (block 28,490,503)
- `pallets_balances_consts_28490503.json` - List
- `pallets_balances_consts_ExistentialDeposit_28490503.json` - Item

### Kusama (block 24,000,000)
- `pallets_balances_consts_24000000.json` - List
- `pallets_balances_consts_ExistentialDeposit_24000000.json` - Item

### Asset Hub Polkadot (block 10,260,000)
- `pallets_balances_consts_10260000.json` - List
- `pallets_balances_consts_ExistentialDeposit_10260000.json` - Item

### Asset Hub Kusama (block 11,152,000)
- `pallets_balances_consts_11152000.json` - List
- `pallets_balances_consts_ExistentialDeposit_11152000.json` - Item

---

## Routes to Add

```rust
// In routes/pallets.rs
.route_registered(
    registry,
    API_VERSION,
    "/pallets/:pallet_id/consts",
    "get",
    get(pallets::pallets_constants),
)
.route_registered(
    registry,
    API_VERSION,
    "/pallets/:pallet_id/consts/:constant_item_id",
    "get",
    get(pallets::pallets_constant_item),
)
```

---

## Checklist

- [ ] Create `constants.rs` handler
- [ ] Add response types
- [ ] Implement V14-V16 extraction
- [ ] Implement V9-V13 extraction
- [ ] Add `useRcBlock` support
- [ ] Register routes
- [ ] Capture fixtures from Sidecar
- [ ] Add test config entries
- [ ] Add unit tests
- [ ] Run fmt/clippy
- [ ] Test all 4 chains

---

## Testing Configuration

### Sidecar Ports (Historical Data)
| Chain | Port |
|-------|------|
| Polkadot | 8070 |
| Kusama | 8071 |
| Asset Hub Polkadot | 8072 |
| Asset Hub Kusama | 8073 |

### PAPI Ports (polkadot-rest-api)
| Chain | Port |
|-------|------|
| Polkadot | 8080 |
| Kusama | 8081 |
| Asset Hub Polkadot | 8082 |
| Asset Hub Kusama | 8083 |

### Capturing Fixtures from Sidecar

```bash
# Polkadot - List constants
curl "http://localhost:8070/pallets/Balances/consts?at=28490503" | jq . > tests/fixtures/polkadot/pallets_balances_consts_28490503.json

# Polkadot - Single constant
curl "http://localhost:8070/pallets/Balances/consts/ExistentialDeposit?at=28490503" | jq . > tests/fixtures/polkadot/pallets_balances_consts_ExistentialDeposit_28490503.json

# Kusama - List constants
curl "http://localhost:8071/pallets/Balances/consts?at=24000000" | jq . > tests/fixtures/kusama/pallets_balances_consts_24000000.json

# Kusama - Single constant
curl "http://localhost:8071/pallets/Balances/consts/ExistentialDeposit?at=24000000" | jq . > tests/fixtures/kusama/pallets_balances_consts_ExistentialDeposit_24000000.json

# Asset Hub Polkadot - List constants
curl "http://localhost:8072/pallets/Balances/consts?at=10260000" | jq . > tests/fixtures/asset-hub-polkadot/pallets_balances_consts_10260000.json

# Asset Hub Polkadot - Single constant
curl "http://localhost:8072/pallets/Balances/consts/ExistentialDeposit?at=10260000" | jq . > tests/fixtures/asset-hub-polkadot/pallets_balances_consts_ExistentialDeposit_10260000.json

# Asset Hub Kusama - List constants
curl "http://localhost:8073/pallets/Balances/consts?at=11152000" | jq . > tests/fixtures/asset-hub-kusama/pallets_balances_consts_11152000.json

# Asset Hub Kusama - Single constant
curl "http://localhost:8073/pallets/Balances/consts/ExistentialDeposit?at=11152000" | jq . > tests/fixtures/asset-hub-kusama/pallets_balances_consts_ExistentialDeposit_11152000.json
```

### Running Tests

Run tests for a specific chain with `API_URL` environment variable:

```bash
# Test against Polkadot PAPI instance on port 8080
API_URL=http://localhost:8080 cargo test --package integration_tests --test historical test_historical_polkadot -- --nocapture

# Test against Kusama PAPI instance on port 8081
API_URL=http://localhost:8081 cargo test --package integration_tests --test historical test_historical_kusama -- --nocapture

# Test against Asset Hub Polkadot PAPI instance on port 8082
API_URL=http://localhost:8082 cargo test --package integration_tests --test historical test_historical_asset_hub_polkadot -- --nocapture

# Test against Asset Hub Kusama PAPI instance on port 8083
API_URL=http://localhost:8083 cargo test --package integration_tests --test historical test_historical_asset_hub_kusama -- --nocapture
```

### Test Config Entries to Add

Add these to `test_config.json` under `historical_tests`:

```json
// In "polkadot" array:
{
  "endpoint": "/v1/pallets/{palletId}/consts",
  "pallet_id": "Balances",
  "query_params": { "at": "28490503" },
  "fixture_path": "polkadot/pallets_balances_consts_28490503.json",
  "description": "Test pallets constants list for Balances at Polkadot block 28,490,503"
},
{
  "endpoint": "/v1/pallets/{palletId}/consts/{constantItemId}",
  "pallet_id": "Balances",
  "constant_item_id": "ExistentialDeposit",
  "query_params": { "at": "28490503" },
  "fixture_path": "polkadot/pallets_balances_consts_ExistentialDeposit_28490503.json",
  "description": "Test pallets constant item ExistentialDeposit for Balances at Polkadot block 28,490,503"
}
```

---

## Reviews from Previous PR to Address

- reviews in previous PR that need to be addresses in this one - 

Just a note/diff while connected to PAH in case no consts were found:

In Sidecar when no consts are found in a pallet, an error message with code 400 is returned (line of code):

no queryable constants items found for palletId "assetTxPayment"
example request: http://localhost:8045/pallets/AssetTxPayment/consts?at=10072050

In polkadot-rest-api, a valid response is returned with the field items as an empty vector:

{
  "at": {
    "hash": "0xf5b3e208e39dadf736ef95d91c36d010508f8344fcb13ccac67b5495b57b3508",
    "height": "10072050"
  },
  "pallet": "assettxpayment",
  "palletIndex": "13",
  "items": []
}
example request : http://localhost:8080/v1/pallets/AssetTxPayment/consts?at=10072050

Imod7
Imod7 reviewed 5 days ago
Imod7
left a comment
• 
We also return different error codes in case a pallet was not found:

In Sidecar when the pallet is not found, an error message with code 400 is returned (line of code):

{
  "code": 400,
  "message": "\"convictionVoting\" was not recognized as a queryable pallet.",
  "stack": "BadRequestError: \"convictionVoting\" was not recognized as a queryable pallet.\n    at PalletsConstantsService.findPalletMeta (/Users/.../substrate-api-sidecar/build/src/services/AbstractPalletsService.js:71:19)\n    at PalletsConstantsService.fetchConstants (/Users/.../substrate-api-sidecar/build/src/services/pallets/PalletsConstantsService.js:50:50)\n    at PalletsConstantsController.getConsts (....substrate-api-sidecar/build/src/controllers/pallets/PalletsConstsController.js:117:51)\n    at process.processTicksAndRejections (....substrate-api-sidecar/build/src/controllers/AbstractController.js:313:9",
  "level": "error"
}
example request (while connected to Polkadot): http://localhost:8045/pallets/ConvictionVoting/consts?at=10372050

In polkadot-rest-api, an error code 404 is returned with message:

{
"error": "Pallet not found: ConvictionVoting"
}
example request : http://localhost:8080/v1/pallets/ConvictionVoting/consts?at=10372050

@Imod7 Imod7 mentioned this pull request 5 days ago
Query Param onlyIds is not working as expected #132
Open
@Imod7
Imod7
commented
5 days ago
Query param onlyIds is not working as expected.

Tracking this in this issue Query Param onlyIds is not working as expected #132
Low priority fix and should not be a blocker for merging this PR.



What is the relationship between this and

https://github.com/paritytech/polkadot-rest-api/pull/124/commits
https://github.com/paritytech/polkadot-rest-api/pull/120/commits
I see shared commits. I'm guessing these should be reviewed in order then? If so you make that obvious at the top of the PR and potentially mark the latter PRs as drafts? I just reviewed this one not realizing it builds off the other 2



we will have a single PR for this entire endpoint 