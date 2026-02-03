# Remaining Endpoints Overview

> **Internal Document** - Not for commit

## Summary

| Priority | Endpoint Group | Endpoints | Complexity | Est. Time |
|----------|----------------|-----------|------------|-----------|
| 1 | Constants | 2 | Medium | 1 day |
| 2 | Pool Assets | 1 | Low | 0.5 day |
| 3 | RC Variants | 11 | Low (after merges) | 1 day |

---

## Group 1: Constants Endpoints

### Endpoints
1. `GET /v1/pallets/{palletId}/consts` - List all constants
2. `GET /v1/pallets/{palletId}/consts/{constantItemId}` - Get specific constant

### Reference
- Sidecar: `src/services/pallets/PalletsConstantsService.ts`
- Similar to: errors, events, dispatchables endpoints

### Implementation Notes
- Extract from metadata `pallet.constants` array
- Decode constant values using type registry
- Support metadata V9-V16 (same pattern as errors/events)

---

## Group 2: Pool Assets Endpoint

### Endpoints
1. `GET /v1/pallets/pool-assets/{assetId}/asset-info` - Get pool asset info

### Reference
- Sidecar: Similar to `PalletsAssetsService.ts`
- Nearly identical to `/pallets/assets/{assetId}/asset-info`

### Implementation Notes
- Query `PoolAssets::Asset` and `PoolAssets::Metadata` storage
- Same response structure as regular assets
- Only available on Asset Hub chains

---

## Group 3: RC (Relay Chain) Variants

> **Wait for base endpoint PRs to merge first**

### Endpoints (11 total)
All query relay chain from Asset Hub:

| Base Endpoint | RC Variant |
|---------------|------------|
| `/pallets/on-going-referenda` | `/rc/pallets/on-going-referenda` |
| `/pallets/{palletId}/consts` | `/rc/pallets/{palletId}/consts` |
| `/pallets/{palletId}/consts/{id}` | `/rc/pallets/{palletId}/consts/{id}` |
| `/pallets/{palletId}/dispatchables` | `/rc/pallets/{palletId}/dispatchables` |
| `/pallets/{palletId}/dispatchables/{id}` | `/rc/pallets/{palletId}/dispatchables/{id}` |
| `/pallets/{palletId}/errors` | `/rc/pallets/{palletId}/errors` |
| `/pallets/{palletId}/errors/{id}` | `/rc/pallets/{palletId}/errors/{id}` |
| `/pallets/{palletId}/events` | `/rc/pallets/{palletId}/events` |
| `/pallets/{palletId}/events/{id}` | `/rc/pallets/{palletId}/events/{id}` |
| `/pallets/{palletId}/storage` | `/rc/pallets/{palletId}/storage` |
| `/pallets/{palletId}/storage/{id}` | `/rc/pallets/{palletId}/storage/{id}` |

### Implementation Notes
- Thin wrappers calling existing handlers
- Use `state.get_relay_chain_client()` instead of `state.client`
- No `useRcBlock` parameter (already querying RC)
- Only available when relay chain connection configured

---

## Execution Plan

### Thread 1: Constants Endpoints
```
Branch: feat/pallets-constants
Files:
  - crates/server/src/handlers/pallets/constants.rs (new)
  - crates/server/src/handlers/pallets/mod.rs (update)
  - crates/server/src/routes/pallets.rs (update)
  - crates/integration_tests/tests/config/test_config.json (update)
  - fixtures for all 4 chains
```

### Thread 2: Pool Assets Endpoint
```
Branch: feat/pallets-pool-assets
Files:
  - crates/server/src/handlers/pallets/pool_assets.rs (new)
  - crates/server/src/handlers/pallets/mod.rs (update)
  - crates/server/src/routes/pallets.rs (update)
  - crates/integration_tests/tests/config/test_config.json (update)
  - fixtures for Asset Hub chains only
```

### Thread 3: RC Endpoints (after merges)
```
Branch: feat/rc-pallets-endpoints
Files:
  - crates/server/src/routes/rc_pallets.rs (new)
  - crates/server/src/routes/mod.rs (update)
  - Update handlers to accept optional relay chain client
```

---

## Status Tracking

- [ ] Constants list endpoint
- [ ] Constants item endpoint
- [ ] Pool assets endpoint
- [ ] RC endpoints (blocked on merges)
