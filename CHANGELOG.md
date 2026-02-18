# Changelog

All notable changes to this project will be documented in this file.

See [standard-version](https://github.com/conventional-changelog/standard-version) for commit guidelines.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [1.0.0-beta.0](https://github.com/paritytech/polkadot-rest-api/compare/38ef4bbc..01da4abb) (2026-02-18)

### Feat

- Add useRcBlockFormat query param to control response shape ([#195](https://github.com/paritytech/polkadot-rest-api/pull/195)) ([30be286f](https://github.com/paritytech/polkadot-rest-api/commit/30be286f))
  - Adds `useRcBlockFormat=object` to return single-object responses instead of arrays
  - Changed default format to accept `format=object` ([#203](https://github.com/paritytech/polkadot-rest-api/pull/203))
- Add OpenAPI documentation with utoipa and custom UI ([#190](https://github.com/paritytech/polkadot-rest-api/pull/190)) ([b227e363](https://github.com/paritytech/polkadot-rest-api/commit/b227e363))
- UseEvmFormat for /blocks endpoints ([#178](https://github.com/paritytech/polkadot-rest-api/pull/178)) ([532fcf2a](https://github.com/paritytech/polkadot-rest-api/commit/532fcf2a))
- Add useRcBlock for /pallets endpoints ([#191](https://github.com/paritytech/polkadot-rest-api/pull/191)) ([849c9b12](https://github.com/paritytech/polkadot-rest-api/commit/849c9b12))
- Add remaining rc endpoints ([#192](https://github.com/paritytech/polkadot-rest-api/pull/192)) ([7b1d9952](https://github.com/paritytech/polkadot-rest-api/commit/7b1d9952))
- Add /accounts/{accountId}/foreign-asset-balances ([#189](https://github.com/paritytech/polkadot-rest-api/pull/189)) ([df5af2ea](https://github.com/paritytech/polkadot-rest-api/commit/df5af2ea))
- UseRcBlock for /blocks endpoints ([#177](https://github.com/paritytech/polkadot-rest-api/pull/177)) ([c6f8c299](https://github.com/paritytech/polkadot-rest-api/commit/c6f8c299))
  - Maps relay chain blocks to Asset Hub blocks for cross-chain querying
- Add `coretime/overview` endpoint ([#187](https://github.com/paritytech/polkadot-rest-api/pull/187)) ([4604b3ce](https://github.com/paritytech/polkadot-rest-api/commit/4604b3ce))
- Implement /pallets/on-going-referenda endpoint ([#141](https://github.com/paritytech/polkadot-rest-api/pull/141)) ([48760e5b](https://github.com/paritytech/polkadot-rest-api/commit/48760e5b))
- Implement /pallets/{palletId}/storage endpoint ([#120](https://github.com/paritytech/polkadot-rest-api/pull/120)) ([84f79cb5](https://github.com/paritytech/polkadot-rest-api/commit/84f79cb5))
- Add `transaction/metadata-blob` ([#184](https://github.com/paritytech/polkadot-rest-api/pull/184)) ([7c9ad296](https://github.com/paritytech/polkadot-rest-api/commit/7c9ad296))
- Add `rc/runtime/*` endpoints ([#186](https://github.com/paritytech/polkadot-rest-api/pull/186)) ([102bc413](https://github.com/paritytech/polkadot-rest-api/commit/102bc413))
- Add /pallets/{palletId}/errors endpoints ([#146](https://github.com/paritytech/polkadot-rest-api/pull/146)) ([8230220b](https://github.com/paritytech/polkadot-rest-api/commit/8230220b))
- Implement /pallets/{palletId}/events endpoints ([#144](https://github.com/paritytech/polkadot-rest-api/pull/144)) ([5fc787d4](https://github.com/paritytech/polkadot-rest-api/commit/5fc787d4))
- Implement /pallets/foreign-assets endpoint ([#145](https://github.com/paritytech/polkadot-rest-api/pull/145)) ([e4b3486c](https://github.com/paritytech/polkadot-rest-api/commit/e4b3486c))
- Add `coretime/info` endpoint ([#175](https://github.com/paritytech/polkadot-rest-api/pull/175)) ([2fc6801c](https://github.com/paritytech/polkadot-rest-api/commit/2fc6801c))
- Add `/paras/{number}/inclusion` endpoint ([#168](https://github.com/paritytech/polkadot-rest-api/pull/168)) ([013a24b8](https://github.com/paritytech/polkadot-rest-api/commit/013a24b8))
- /rc/blocks/{blockId}/extrinsics/{extrinsicIndex} ([#164](https://github.com/paritytech/polkadot-rest-api/pull/164)) ([c5b3a143](https://github.com/paritytech/polkadot-rest-api/commit/c5b3a143))
- Implement /v1/pallets/pool-assets/{assetId}/asset-info endpoint ([#155](https://github.com/paritytech/polkadot-rest-api/pull/155)) ([bf947fbb](https://github.com/paritytech/polkadot-rest-api/commit/bf947fbb))
- Add `coretime/renewals` endpoint ([#174](https://github.com/paritytech/polkadot-rest-api/pull/174)) ([48250f26](https://github.com/paritytech/polkadot-rest-api/commit/48250f26))
- Add /pallets/nomination-pools/ endpoints ([#143](https://github.com/paritytech/polkadot-rest-api/pull/143)) ([d72cea9c](https://github.com/paritytech/polkadot-rest-api/commit/d72cea9c))
- Add /pallets/{palletId}/dispatchables endpoints ([#147](https://github.com/paritytech/polkadot-rest-api/pull/147)) ([94963b49](https://github.com/paritytech/polkadot-rest-api/commit/94963b49))
- /rc/blocks/{blockId}/header ([#165](https://github.com/paritytech/polkadot-rest-api/pull/165)) ([a22fc399](https://github.com/paritytech/polkadot-rest-api/commit/a22fc399))
- Add `coretime/regions` endpoint ([#171](https://github.com/paritytech/polkadot-rest-api/pull/171)) ([b4e7fab2](https://github.com/paritytech/polkadot-rest-api/commit/b4e7fab2))
- /rc/blocks/{blockId}/para-inclusions ([#137](https://github.com/paritytech/polkadot-rest-api/pull/137)) ([8356c407](https://github.com/paritytech/polkadot-rest-api/commit/8356c407))
- Asset conversion endpoints ([#134](https://github.com/paritytech/polkadot-rest-api/pull/134)) ([9515da52](https://github.com/paritytech/polkadot-rest-api/commit/9515da52))
- Add `/coretime/reservations` endpoint ([#169](https://github.com/paritytech/polkadot-rest-api/pull/169)) ([d89b4848](https://github.com/paritytech/polkadot-rest-api/commit/d89b4848))
- /rc/blocks/{blockId} ([#166](https://github.com/paritytech/polkadot-rest-api/pull/166)) ([f07d48fd](https://github.com/paritytech/polkadot-rest-api/commit/f07d48fd))
- Add /pallets/{palletId}/consts endpoints ([#157](https://github.com/paritytech/polkadot-rest-api/pull/157)) ([8de0607f](https://github.com/paritytech/polkadot-rest-api/commit/8de0607f))
- Add `coretime/leases` endpoint ([#160](https://github.com/paritytech/polkadot-rest-api/pull/160)) ([a2fbed6e](https://github.com/paritytech/polkadot-rest-api/commit/a2fbed6e))
- Add `/transaction/material/{metadataVersion}` and `rc/transaction/material/{metadataVersion}` ([#162](https://github.com/paritytech/polkadot-rest-api/pull/162)) ([62a1de7e](https://github.com/paritytech/polkadot-rest-api/commit/62a1de7e))
- /accounts/* and /rc/accounts/* ([#135](https://github.com/paritytech/polkadot-rest-api/pull/135)) ([a4391036](https://github.com/paritytech/polkadot-rest-api/commit/a4391036))
  - Includes balance-info, vesting-info, staking-info, staking-payouts, and proxy-info
- Node endpoints ([#119](https://github.com/paritytech/polkadot-rest-api/pull/119)) ([9a57b01f](https://github.com/paritytech/polkadot-rest-api/commit/9a57b01f))
  - `/node/version`, `/node/roles`, `/node/network`, `/node/transaction-pool`
- /rc/blocks/head ([#154](https://github.com/paritytech/polkadot-rest-api/pull/154)) ([3bdd3e48](https://github.com/paritytech/polkadot-rest-api/commit/3bdd3e48))
- Add `transaction/material` and `/rc/transaction/material` endpoints ([#161](https://github.com/paritytech/polkadot-rest-api/pull/161)) ([09621050](https://github.com/paritytech/polkadot-rest-api/commit/09621050))
- /rc/blocks/head/header ([#139](https://github.com/paritytech/polkadot-rest-api/pull/139)) ([6c13a65b](https://github.com/paritytech/polkadot-rest-api/commit/6c13a65b))
- /rc/blocks ([#133](https://github.com/paritytech/polkadot-rest-api/pull/133)) ([d489d0f5](https://github.com/paritytech/polkadot-rest-api/commit/d489d0f5))
- Add `transaction/fee-estimate` and `rc/transaction/fee-estimate` ([#158](https://github.com/paritytech/polkadot-rest-api/pull/158)) ([5873fc81](https://github.com/paritytech/polkadot-rest-api/commit/5873fc81))
- /blocks/{blockId}/para-inclusions ([#130](https://github.com/paritytech/polkadot-rest-api/pull/130)) ([6683d4f5](https://github.com/paritytech/polkadot-rest-api/commit/6683d4f5))
- /rc/blocks/{blockId}/extrinsics-raw ([#136](https://github.com/paritytech/polkadot-rest-api/pull/136)) ([9ec4a21f](https://github.com/paritytech/polkadot-rest-api/commit/9ec4a21f))
- Add `transaction/dry-run` and `rc/transaction/dry-run` ([#153](https://github.com/paritytech/polkadot-rest-api/pull/153)) ([12e96747](https://github.com/paritytech/polkadot-rest-api/commit/12e96747))
- Add `/transaction` endpoint ([#152](https://github.com/paritytech/polkadot-rest-api/pull/152)) ([17f39b72](https://github.com/paritytech/polkadot-rest-api/commit/17f39b72))
- `pallets/staking/validators` and `rc/pallets/staking/validators` ([#138](https://github.com/paritytech/polkadot-rest-api/pull/138)) ([9b8796a0](https://github.com/paritytech/polkadot-rest-api/commit/9b8796a0))
- /blocks/{blockId}/extrinsics/{extrinsicIndex} ([#127](https://github.com/paritytech/polkadot-rest-api/pull/127)) ([65b21f8f](https://github.com/paritytech/polkadot-rest-api/commit/65b21f8f))
- /blocks/{blockId}/header ([#125](https://github.com/paritytech/polkadot-rest-api/pull/125)) ([fbf7970a](https://github.com/paritytech/polkadot-rest-api/commit/fbf7970a))
- Add support for env files ([#121](https://github.com/paritytech/polkadot-rest-api/pull/121)) ([455d84c6](https://github.com/paritytech/polkadot-rest-api/commit/455d84c6))
- /blocks/{blockId}/extrinsics-raw ([#129](https://github.com/paritytech/polkadot-rest-api/pull/129)) ([65a92ef4](https://github.com/paritytech/polkadot-rest-api/commit/65a92ef4))
- `pallets/staking/progress` and `rc/pallets/staking/progress` ([#131](https://github.com/paritytech/polkadot-rest-api/pull/131)) ([5c112593](https://github.com/paritytech/polkadot-rest-api/commit/5c112593))
- Implement /pallets/assets/{assetId}/asset-info ([#122](https://github.com/paritytech/polkadot-rest-api/pull/122)) ([c4cc5cd5](https://github.com/paritytech/polkadot-rest-api/commit/c4cc5cd5))
- /blocks endpoint ([#123](https://github.com/paritytech/polkadot-rest-api/pull/123)) ([03cbcd88](https://github.com/paritytech/polkadot-rest-api/commit/03cbcd88))
- Add v1/capabilities endpoint ([#116](https://github.com/paritytech/polkadot-rest-api/pull/116)) ([e69b29ce](https://github.com/paritytech/polkadot-rest-api/commit/e69b29ce))
- Add useRcBlock ([#100](https://github.com/paritytech/polkadot-rest-api/pull/100)) ([c05cf0bd](https://github.com/paritytech/polkadot-rest-api/commit/c05cf0bd))
  - Query Asset Hub state using a relay chain block reference
- Add chains-config-v1 ([#114](https://github.com/paritytech/polkadot-rest-api/pull/114)) ([046879dd](https://github.com/paritytech/polkadot-rest-api/commit/046879dd))
- Add runtime/metadata endpoints ([#95](https://github.com/paritytech/polkadot-rest-api/pull/95)) ([6af48947](https://github.com/paritytech/polkadot-rest-api/commit/6af48947))
- Add WebSocket reconnection logic ([#110](https://github.com/paritytech/polkadot-rest-api/pull/110)) ([62782248](https://github.com/paritytech/polkadot-rest-api/commit/62782248))
- Add /blocks/head endpoint ([#106](https://github.com/paritytech/polkadot-rest-api/pull/106)) ([dcee81a5](https://github.com/paritytech/polkadot-rest-api/commit/dcee81a5))
- Add container metrics to the supplied grafana dashboard ([#102](https://github.com/paritytech/polkadot-rest-api/pull/102)) ([4be753aa](https://github.com/paritytech/polkadot-rest-api/commit/4be753aa))
- DecodeXcmMsgs query param for /blocks/{blockId} ([#94](https://github.com/paritytech/polkadot-rest-api/pull/94)) ([8ff74869](https://github.com/paritytech/polkadot-rest-api/commit/8ff74869))
- Add block-pruning to the docker compose node ([#101](https://github.com/paritytech/polkadot-rest-api/pull/101)) ([49360bad](https://github.com/paritytech/polkadot-rest-api/commit/49360bad))
- Add /runtime/code endpoint to retrieve WASM blob ([#93](https://github.com/paritytech/polkadot-rest-api/pull/93)) ([8ea4b6cb](https://github.com/paritytech/polkadot-rest-api/commit/8ea4b6cb))
- Grafana dashboard v1 ([#96](https://github.com/paritytech/polkadot-rest-api/pull/96)) ([db26e3cd](https://github.com/paritytech/polkadot-rest-api/commit/db26e3cd))
- Add dynamic route registry and root endpoint ([#89](https://github.com/paritytech/polkadot-rest-api/pull/89)) ([87106aaa](https://github.com/paritytech/polkadot-rest-api/commit/87106aaa))
- CI Pipeline to prepare RAPI for deployments ([#83](https://github.com/paritytech/polkadot-rest-api/pull/83)) ([4027c034](https://github.com/paritytech/polkadot-rest-api/commit/4027c034))
- Docker and docker compose ([#78](https://github.com/paritytech/polkadot-rest-api/pull/78)) ([9624305c](https://github.com/paritytech/polkadot-rest-api/commit/9624305c))
- Add `finalizedKey` to `/blocks/{blockId}` ([#75](https://github.com/paritytech/polkadot-rest-api/pull/75)) ([e9ba2e86](https://github.com/paritytech/polkadot-rest-api/commit/e9ba2e86))
- Add `noFees` query param to /blocks/* ([#74](https://github.com/paritytech/polkadot-rest-api/pull/74)) ([cc46c539](https://github.com/paritytech/polkadot-rest-api/commit/cc46c539))
- Add extrinsicDocs, and eventDocs query params to /blocks/{blockId} ([#72](https://github.com/paritytech/polkadot-rest-api/pull/72)) ([d597186e](https://github.com/paritytech/polkadot-rest-api/commit/d597186e))
- Http logger ([#68](https://github.com/paritytech/polkadot-rest-api/pull/68)) ([4054f283](https://github.com/paritytech/polkadot-rest-api/commit/4054f283))
- Add visitor implementation from subxt ([#52](https://github.com/paritytech/polkadot-rest-api/pull/52)) ([294cf8e8](https://github.com/paritytech/polkadot-rest-api/commit/294cf8e8))
- Populate events in /blocks/{blockId} ([#36](https://github.com/paritytech/polkadot-rest-api/pull/36)) ([8b083d32](https://github.com/paritytech/polkadot-rest-api/commit/8b083d32))
- Add `extrinsics` field to `/blocks/{blockId}` ([#31](https://github.com/paritytech/polkadot-rest-api/pull/31)) ([84bac5de](https://github.com/paritytech/polkadot-rest-api/commit/84bac5de))
- Loki support for log aggregation ([#42](https://github.com/paritytech/polkadot-rest-api/pull/42)) ([89134ae5](https://github.com/paritytech/polkadot-rest-api/commit/89134ae5))
- Prometheus metrics ([#40](https://github.com/paritytech/polkadot-rest-api/pull/40)) ([b81abc25](https://github.com/paritytech/polkadot-rest-api/commit/b81abc25))
- Add /blocks/head/header ([#26](https://github.com/paritytech/polkadot-rest-api/pull/26)) ([6c9c0924](https://github.com/paritytech/polkadot-rest-api/commit/6c9c0924))
- Add `/blocks/{blockId}` with a partial response no query params ([#24](https://github.com/paritytech/polkadot-rest-api/pull/24)) ([4358dbc8](https://github.com/paritytech/polkadot-rest-api/commit/4358dbc8))
- Add resolve_block, and `/runtime/node` endpoint ([#21](https://github.com/paritytech/polkadot-rest-api/pull/21)) ([bcc9ff09](https://github.com/paritytech/polkadot-rest-api/commit/bcc9ff09))
- Standardize errors with thiserror, and enums ([#19](https://github.com/paritytech/polkadot-rest-api/pull/19)) ([7cafa7f0](https://github.com/paritytech/polkadot-rest-api/commit/7cafa7f0))
- Add ChainInfo struct as part of AppState ([#18](https://github.com/paritytech/polkadot-rest-api/pull/18)) ([88c28b64](https://github.com/paritytech/polkadot-rest-api/commit/88c28b64))
- Add URI Path Versioning ([d73c3213](https://github.com/paritytech/polkadot-rest-api/commit/d73c3213))
- Add initial subxt-historic connection ([#10](https://github.com/paritytech/polkadot-rest-api/pull/10)) ([9303d5da](https://github.com/paritytech/polkadot-rest-api/commit/9303d5da))
- Benchmark for /blocks endpoints ([#33](https://github.com/paritytech/polkadot-rest-api/pull/33)) ([c19a2014](https://github.com/paritytech/polkadot-rest-api/commit/c19a2014))
- SAS-compatible environment configuration
  - `SAS_SUBSTRATE_URL`, `SAS_SUBSTRATE_MULTI_CHAIN_URL`, `SAS_EXPRESS_BIND_HOST`, `SAS_EXPRESS_PORT`
  - `SAS_LOG_LEVEL`, `SAS_LOG_JSON`, `SAS_LOG_STRIP_ANSI`
  - `SAS_LOG_WRITE`, `SAS_LOG_WRITE_PATH`, `SAS_LOG_WRITE_MAX_FILE_SIZE`, `SAS_LOG_WRITE_MAX_FILES`
  - `SAS_EXPRESS_KEEP_ALIVE_TIMEOUT`, `SAS_EXPRESS_REQUEST_LIMIT`

### Fix

- Use explicit types for signed extension decoding ([#219](https://github.com/paritytech/polkadot-rest-api/pull/219)) ([006bb876](https://github.com/paritytech/polkadot-rest-api/commit/006bb876))
- Handle multiple AH blocks in assets and staking-progress useRcBlock ([#202](https://github.com/paritytech/polkadot-rest-api/pull/202)) ([76ac7d69](https://github.com/paritytech/polkadot-rest-api/commit/76ac7d69))
- Use chain_getBlock RPC for rc extrinsics-raw endpoint ([#216](https://github.com/paritytech/polkadot-rest-api/pull/216)) ([c23d1781](https://github.com/paritytech/polkadot-rest-api/commit/c23d1781))
- Use correct SS58 prefix for proxy delegate addresses ([#215](https://github.com/paritytech/polkadot-rest-api/pull/215)) ([a265ec9d](https://github.com/paritytech/polkadot-rest-api/commit/a265ec9d))
- Return JSON error for missing required query parameters ([#218](https://github.com/paritytech/polkadot-rest-api/pull/218)) ([bd30d89a](https://github.com/paritytech/polkadot-rest-api/commit/bd30d89a))
- Correct vesting schedule SCALE decoding ([#214](https://github.com/paritytech/polkadot-rest-api/pull/214)) ([7171f349](https://github.com/paritytech/polkadot-rest-api/commit/7171f349))
- Address diff mismatches from #208 ([#213](https://github.com/paritytech/polkadot-rest-api/pull/213)) ([2a84ae24](https://github.com/paritytech/polkadot-rest-api/commit/2a84ae24))
- Update migration & config guides ([#200](https://github.com/paritytech/polkadot-rest-api/pull/200)) ([a092cb9c](https://github.com/paritytech/polkadot-rest-api/commit/a092cb9c))
- Benchmark CI ([#206](https://github.com/paritytech/polkadot-rest-api/pull/206)) ([c3e6a4ff](https://github.com/paritytech/polkadot-rest-api/commit/c3e6a4ff))
- Enable legacy types for Kusama Asset Hub historic blocks ([#197](https://github.com/paritytech/polkadot-rest-api/pull/197)) ([a72e5997](https://github.com/paritytech/polkadot-rest-api/commit/a72e5997))
- Error when path has trailing slash ([#193](https://github.com/paritytech/polkadot-rest-api/pull/193)) ([303ad583](https://github.com/paritytech/polkadot-rest-api/commit/303ad583))
- Remove logging from on_going_referenda ([#188](https://github.com/paritytech/polkadot-rest-api/pull/188)) ([97593986](https://github.com/paritytech/polkadot-rest-api/commit/97593986))
- ci: Fix rpc nodes and fallback nodes ([#159](https://github.com/paritytech/polkadot-rest-api/pull/159)) ([c0796390](https://github.com/paritytech/polkadot-rest-api/commit/c0796390))
- Remove /rc/pallets related endpoints from relay ([#150](https://github.com/paritytech/polkadot-rest-api/pull/150)) ([78ed6940](https://github.com/paritytech/polkadot-rest-api/commit/78ed6940))
- Remove SAS_RELAY_CHAIN_URL in favor of SAS_SUBSTRATE_MULTI_CHAIN_URL ([#148](https://github.com/paritytech/polkadot-rest-api/pull/148)) ([850d2c7e](https://github.com/paritytech/polkadot-rest-api/commit/850d2c7e))
- RewardDestination event decoding ([#126](https://github.com/paritytech/polkadot-rest-api/pull/126)) ([e021481d](https://github.com/paritytech/polkadot-rest-api/commit/e021481d))
- Integrate subxt v0.50.0 ([#118](https://github.com/paritytech/polkadot-rest-api/pull/118)) ([3d33ec40](https://github.com/paritytech/polkadot-rest-api/commit/3d33ec40))
- Add connection logging and timeout with CLI progress ([#113](https://github.com/paritytech/polkadot-rest-api/pull/113)) ([df56cdf3](https://github.com/paritytech/polkadot-rest-api/commit/df56cdf3))
- Enum serialization and array decoding for /blocks/* ([#107](https://github.com/paritytech/polkadot-rest-api/pull/107)) ([c9d5b545](https://github.com/paritytech/polkadot-rest-api/commit/c9d5b545))
- Type-aware JSON decoding for args and reorganize decode modules ([#103](https://github.com/paritytech/polkadot-rest-api/pull/103)) ([96eae742](https://github.com/paritytech/polkadot-rest-api/commit/96eae742))
- Fix middleware so the response_size_bytes metric is populated ([#98](https://github.com/paritytech/polkadot-rest-api/pull/98)) ([cbcc3293](https://github.com/paritytech/polkadot-rest-api/commit/cbcc3293))
- Allow hostnames in config validation for loki addresses ([#97](https://github.com/paritytech/polkadot-rest-api/pull/97)) ([134a6a38](https://github.com/paritytech/polkadot-rest-api/commit/134a6a38))
- Only create one shared client.at() ([#82](https://github.com/paritytech/polkadot-rest-api/pull/82)) ([a73219ac](https://github.com/paritytech/polkadot-rest-api/commit/a73219ac))
- Add `info` with fee calculation for extrinsics ([#64](https://github.com/paritytech/polkadot-rest-api/pull/64)) ([fa917f29](https://github.com/paritytech/polkadot-rest-api/commit/fa917f29))
- Add `system_chainType` RPC call and fix response format for `/v1/runtime/spec` ([#70](https://github.com/paritytech/polkadot-rest-api/pull/70)) ([e5b66f72](https://github.com/paritytech/polkadot-rest-api/commit/e5b66f72))
- Add a retry mechanism in the CI server startup ([#65](https://github.com/paritytech/polkadot-rest-api/pull/65)) ([5c26d7dc](https://github.com/paritytech/polkadot-rest-api/commit/5c26d7dc))
- Ensure decoding diffs are handled for all integration tests ([#63](https://github.com/paritytech/polkadot-rest-api/pull/63)) ([42ff70d0](https://github.com/paritytech/polkadot-rest-api/commit/42ff70d0))
- Add finalized field to get_blocks ([#62](https://github.com/paritytech/polkadot-rest-api/pull/62)) ([8d3f615c](https://github.com/paritytech/polkadot-rest-api/commit/8d3f615c))
- Add success and paysFee fields to extrinsics ([#61](https://github.com/paritytech/polkadot-rest-api/pull/61)) ([a3e2fb98](https://github.com/paritytech/polkadot-rest-api/commit/a3e2fb98))
- Digest log format to match substrate-api-sidecar and restore author extraction ([#59](https://github.com/paritytech/polkadot-rest-api/pull/59)) ([d326b6ba](https://github.com/paritytech/polkadot-rest-api/commit/d326b6ba))
- Ensure empty arrays stay as empty arrays ([#54](https://github.com/paritytech/polkadot-rest-api/pull/54)) ([b9e4b29c](https://github.com/paritytech/polkadot-rest-api/commit/b9e4b29c))
- Ensure authorId is a ss58 address ([#53](https://github.com/paritytech/polkadot-rest-api/pull/53)) ([38a0c044](https://github.com/paritytech/polkadot-rest-api/commit/38a0c044))
- CI Build pipeline changes ([#84](https://github.com/paritytech/polkadot-rest-api/pull/84)) ([c2ba4144](https://github.com/paritytech/polkadot-rest-api/commit/c2ba4144))
- Change docker registry to paritytech ([#85](https://github.com/paritytech/polkadot-rest-api/pull/85)) ([ec1928ce](https://github.com/paritytech/polkadot-rest-api/commit/ec1928ce))
- Update Docker tag format in deploy workflow ([#86](https://github.com/paritytech/polkadot-rest-api/pull/86)) ([2483b9bd](https://github.com/paritytech/polkadot-rest-api/commit/2483b9bd))
- Fix DATE_TAG echo command in deploy.yml ([#87](https://github.com/paritytech/polkadot-rest-api/pull/87)) ([5211f8b3](https://github.com/paritytech/polkadot-rest-api/commit/5211f8b3))

### Chore

- test: Fix integration test author id for aura ([#220](https://github.com/paritytech/polkadot-rest-api/pull/220)) ([01da4abb](https://github.com/paritytech/polkadot-rest-api/commit/01da4abb))
- Bump qs from 6.14.1 to 6.14.2 in /docs ([#201](https://github.com/paritytech/polkadot-rest-api/pull/201)) ([b67c17d4](https://github.com/paritytech/polkadot-rest-api/commit/b67c17d4))
- 2026 headers ([#205](https://github.com/paritytech/polkadot-rest-api/pull/205)) ([62111d6c](https://github.com/paritytech/polkadot-rest-api/commit/62111d6c))
- README disclaimer ([#204](https://github.com/paritytech/polkadot-rest-api/pull/204)) ([1f323499](https://github.com/paritytech/polkadot-rest-api/commit/1f323499))
- Accounts scale value ([#181](https://github.com/paritytech/polkadot-rest-api/pull/181)) ([d47071b0](https://github.com/paritytech/polkadot-rest-api/commit/d47071b0))
- Refactor pallets endpoints ([#196](https://github.com/paritytech/polkadot-rest-api/pull/196)) ([b6e0051b](https://github.com/paritytech/polkadot-rest-api/commit/b6e0051b))
- Add HTTP configuration for polkadot-rest-api ([#194](https://github.com/paritytech/polkadot-rest-api/pull/194)) ([e44c789f](https://github.com/paritytech/polkadot-rest-api/commit/e44c789f))
- Bump time from 0.3.45 to 0.3.47 ([#183](https://github.com/paritytech/polkadot-rest-api/pull/183)) ([25f014f7](https://github.com/paritytech/polkadot-rest-api/commit/25f014f7))
- Bump bytes from 1.11.0 to 1.11.1 ([#179](https://github.com/paritytech/polkadot-rest-api/pull/179)) ([ee4be1e8](https://github.com/paritytech/polkadot-rest-api/commit/ee4be1e8))
- ci: Add RPC fallback for integration tests ([#115](https://github.com/paritytech/polkadot-rest-api/pull/115)) ([7f621ba9](https://github.com/paritytech/polkadot-rest-api/commit/7f621ba9))
- Optimize fee extraction in blocks endpoint ([#112](https://github.com/paritytech/polkadot-rest-api/pull/112)) ([c1ee71da](https://github.com/paritytech/polkadot-rest-api/commit/c1ee71da))
- Split common.rs into processing/ module ([#105](https://github.com/paritytech/polkadot-rest-api/pull/105)) ([6511d7b0](https://github.com/paritytech/polkadot-rest-api/commit/6511d7b0))
- test: Add /runtime/spec fixtures for Kusama and Asset Hubs ([#92](https://github.com/paritytech/polkadot-rest-api/pull/92)) ([2a8ee58e](https://github.com/paritytech/polkadot-rest-api/commit/2a8ee58e))
- Update .gitignore to ignore CLAUDE.md ([#77](https://github.com/paritytech/polkadot-rest-api/pull/77)) ([e9725a23](https://github.com/paritytech/polkadot-rest-api/commit/e9725a23))
- Bump subxt-historic to 0.0.6 ([#71](https://github.com/paritytech/polkadot-rest-api/pull/71)) ([c44ceea5](https://github.com/paritytech/polkadot-rest-api/commit/c44ceea5))
- Refactor the get_blocks module to be better organized across the repo ([#73](https://github.com/paritytech/polkadot-rest-api/pull/73)) ([eac69838](https://github.com/paritytech/polkadot-rest-api/commit/eac69838))
- test: Fix integration tests hanging when server is not running ([#69](https://github.com/paritytech/polkadot-rest-api/pull/69)) ([c70a0a56](https://github.com/paritytech/polkadot-rest-api/commit/c70a0a56))
- test: Improve ahm-info tests to use mockRpcClient ([#67](https://github.com/paritytech/polkadot-rest-api/pull/67)) ([50e34442](https://github.com/paritytech/polkadot-rest-api/commit/50e34442))
- test: Add output per integration test that completes ([#66](https://github.com/paritytech/polkadot-rest-api/pull/66)) ([916c20c2](https://github.com/paritytech/polkadot-rest-api/commit/916c20c2))
- test: Add integration test for polkadot block 10000000 ([#60](https://github.com/paritytech/polkadot-rest-api/pull/60)) ([5806a809](https://github.com/paritytech/polkadot-rest-api/commit/5806a809))
- test: Create in depth diff for integration tests ([#56](https://github.com/paritytech/polkadot-rest-api/pull/56)) ([dcc8df92](https://github.com/paritytech/polkadot-rest-api/commit/dcc8df92))
- test: Add tests for `utils/block.rs`, and `get_spec` with subxt mock rpc client ([#22](https://github.com/paritytech/polkadot-rest-api/pull/22)) ([44d3f2f9](https://github.com/paritytech/polkadot-rest-api/commit/44d3f2f9))
- ci: Add semantic ci check for conventional commits ([#17](https://github.com/paritytech/polkadot-rest-api/pull/17)) ([89a20630](https://github.com/paritytech/polkadot-rest-api/commit/89a20630))
- ci: Add base github actions ([#7](https://github.com/paritytech/polkadot-rest-api/pull/7)) ([bfe2ce27](https://github.com/paritytech/polkadot-rest-api/commit/bfe2ce27))

## Compatibility

Tested against:
- Polkadot
- Kusama
- Westend
- Polkadot Asset Hub
- Kusama Asset Hub
