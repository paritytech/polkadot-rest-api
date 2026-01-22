# Accounts Handlers - Improvement Analysis

Analysis of `handlers/accounts/` and `handlers/rc/accounts/` directories.

## Critical Issues

### 1. Code Duplication
- **RC access logic** duplicated across all handlers (`get_balance_info`, `get_proxy_info`, etc.)
- **Network name mapping** (`get_network_name()`) copy-pasted in `get_convert.rs`, `get_compare.rs`, `get_validate.rs`
- **Asset handlers** (`get_asset_balances.rs`, `get_pool_asset_balances.rs`) share identical RC block handling

**Fix**: Extract into shared `utils/relay_chain.rs` module.

### 2. Inconsistent Error Handling
- `get_balance_info.rs:36` wraps errors: `.map_err(|_| BalanceInfoError::InvalidAddress(...))`
- `get_asset_balances.rs:36` propagates raw: `validate_and_parse_address(&account_id)?`
- `types.rs:186` uses catch-all `_` pattern, silently returning 500 for distinct errors

**Fix**: Standardize error mapping across all handlers.

## Medium Priority

### 3. Missing Validation
- `StakingPayoutsQueryParams.depth` - no upper bound (docs say "less than history depth")
- `BalanceInfoQueryParams.token` - no length limit
- `get_compare.rs` - 30 address limit but no request size validation

### 4. Error Context Loss
```rust
// get_asset_balances.rs:78 - misleading error
Err(_e) => return Err(AssetBalancesError::AssetsPalletNotAvailable)
```
Network errors reported as "pallet not available".

### 5. Debug Logging in Production
```rust
// Found in ALL handlers
println!("Fetching balance info for account {:?} at block {}", ...);
```
**Fix**: Use `tracing` crate with proper log levels.

### 6. Type Inconsistencies
- `ss58_prefix`: `Option<String>` in `AccountValidateResponse`, `u16` in `AccountConvertResponse`
- RC fields (`rc_block_hash`, `rc_block_number`) present in some responses but not others

### 7. Performance
- `utils/assets.rs:21-41` - `query_all_assets_id()` fetches all assets without caching
- No pagination for chains with thousands of assets

## Low Priority

### 8. Naming Inconsistencies
- `handle_use_rc_block()` vs `get_relay_chain_access()` for similar operations
- Inconsistent error variant naming

### 9. Unnecessary Code
```rust
// get_asset_balances.rs:87-92 - redundant loop
let mut assets = Vec::new();
for balance in balances { assets.push(balance); }
// Should be: let assets = balances;
```

### 10. Edge Case Handling
```rust
// get_balance_info.rs:145-147 - ambiguous empty response
if ah_blocks.is_empty() { return Ok(Json(json!([])).into_response()); }
```
Clients cannot distinguish "no blocks" from "empty results".

## Recommended Actions

| Priority | Action | Effort |
|----------|--------|--------|
| 1 | Extract shared RC access utilities | Medium |
| 2 | Standardize error handling patterns | Medium |
| 3 | Replace println with tracing | Low |
| 4 | Add validation to query params | Low |
| 5 | Implement asset ID caching | Medium |
| 6 | Align response type structures | Medium |
