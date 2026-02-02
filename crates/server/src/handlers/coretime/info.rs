use crate::handlers::coretime::common::{
    AtResponse, CoretimeError, CoretimeQueryParams, has_broker_pallet, has_coretime_pallet,
};
use crate::state::AppState;
use crate::utils::{BlockId, resolve_block};
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use config::ChainType;
use parity_scale_codec::{Decode, Encode};
use primitive_types::H256;
use serde::Serialize;
use std::str::FromStr;
use subxt::{SubstrateConfig, client::OnlineClientAtBlock};

// ============================================================================
// Response Types - Coretime Chain (Broker Pallet)
// ============================================================================

/// Response for GET /coretime/info endpoint on coretime chains.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoretimeInfoResponse {
    /// Block context (hash and height).
    pub at: AtResponse,
    /// Broker configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configuration: Option<ConfigurationInfo>,
    /// Current region timing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_region: Option<CurrentRegionInfo>,
    /// Core availability and pricing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cores: Option<CoresInfo>,
    /// Sale phase information.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<PhaseInfo>,
}

/// Broker configuration information.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigurationInfo {
    /// Length of a region in timeslices.
    pub region_length: u32,
    /// Length of the interlude period in relay blocks.
    pub interlude_length: u32,
    /// Length of the leadin period in relay blocks.
    pub leadin_length: u32,
    /// Number of relay chain blocks per timeslice.
    pub relay_blocks_per_timeslice: u32,
}

/// Current region timing information.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentRegionInfo {
    /// Start timeslice of the current region.
    pub start: Option<u32>,
    /// End timeslice of the current region.
    pub end: Option<u32>,
}

/// Core availability and pricing information.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoresInfo {
    /// Number of cores available for purchase.
    pub available: u32,
    /// Number of cores already sold.
    pub sold: u32,
    /// Total cores offered for sale.
    pub total: u32,
    /// Current price per core (as string for large numbers).
    pub current_core_price: String,
    /// Price at which cores sold out (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sellout_price: Option<String>,
    /// First core index in the sale.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_core: Option<u32>,
}

/// Sale phase information.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhaseInfo {
    /// Name of the current phase.
    pub current_phase: String,
    /// Configuration of all phases.
    pub config: Vec<PhaseConfig>,
}

/// Configuration for a single sale phase.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhaseConfig {
    /// Name of the phase.
    pub phase_name: String,
    /// Last relay block of this phase.
    pub last_relay_block: u32,
    /// Last timeslice of this phase.
    pub last_timeslice: u32,
}

// ============================================================================
// Response Types - Relay Chain (Coretime Pallet)
// ============================================================================

/// Response for GET /coretime/info endpoint on relay chains.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoretimeRelayInfoResponse {
    /// Block context (hash and height).
    pub at: AtResponse,
    /// Parachain ID of the coretime broker chain.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broker_id: Option<u32>,
    /// Pallet version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pallet_version: Option<u32>,
    /// Maximum historical revenue blocks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_historical_revenue: Option<u32>,
}

// ============================================================================
// Internal SCALE Decode Types
// ============================================================================

/// ConfigRecord from Broker pallet.
/// Note: Uses u32 for block numbers since relay block numbers fit in u32.
#[derive(Debug, Clone, Decode, Encode)]
struct ConfigRecord {
    advance_notice: u32,
    interlude_length: u32,
    leadin_length: u32,
    region_length: u32,
    ideal_bulk_proportion: u32, // Perbill (parts per billion)
    limit_cores_offered: Option<u16>,
    renewal_bump: u32, // Perbill
    contribution_timeout: u32,
}

/// SaleInfoRecord from Broker pallet.
#[derive(Debug, Clone, Decode, Encode)]
struct SaleInfoRecord {
    sale_start: u32,
    leadin_length: u32,
    end_price: u128,
    region_begin: u32,
    region_end: u32,
    ideal_cores_sold: u16,
    cores_offered: u16,
    first_core: u16,
    sellout_price: Option<u128>,
    cores_sold: u16,
}

/// StatusRecord from Broker pallet.
#[derive(Debug, Clone, Decode, Encode)]
struct StatusRecord {
    core_count: u16,
    private_pool_size: u32,
    system_pool_size: u32,
    last_committed_timeslice: u32,
    last_timeslice: u32,
}

// ============================================================================
// Main Handler
// ============================================================================

/// Handler for GET /coretime/info endpoint.
///
/// Returns coretime system information. The response structure differs based on chain type:
/// - Relay chains: broker ID, pallet version, max historical revenue
/// - Coretime chains: configuration, sale info, core availability, phase info
///
/// Query Parameters:
/// - at: Optional block number or hash to query at (defaults to latest finalized)
pub async fn coretime_info(
    State(state): State<AppState>,
    Query(params): Query<CoretimeQueryParams>,
) -> Result<Response, CoretimeError> {
    // Parse the block ID if provided
    let block_id = match &params.at {
        None => None,
        Some(at_str) => Some(at_str.parse::<BlockId>()?),
    };

    // Resolve the block
    let resolved_block = resolve_block(&state, block_id).await?;

    // Get client at the resolved block hash
    let block_hash =
        H256::from_str(&resolved_block.hash).map_err(|_| CoretimeError::InvalidBlockHash)?;
    let client_at_block = state.client.at_block(block_hash).await?;

    let at = AtResponse {
        hash: resolved_block.hash,
        height: resolved_block.number.to_string(),
    };

    // Route based on chain type
    match state.chain_info.chain_type {
        ChainType::Coretime => {
            // Coretime chain - return full broker info
            handle_coretime_chain_info(&client_at_block, at, resolved_block.number).await
        }
        ChainType::Relay => {
            // Relay chain - return minimal coretime pallet info
            handle_relay_chain_info(&client_at_block, at).await
        }
        _ => {
            // Other chain types - check if broker pallet exists
            if has_broker_pallet(&client_at_block) {
                handle_coretime_chain_info(&client_at_block, at, resolved_block.number).await
            } else if has_coretime_pallet(&client_at_block) {
                handle_relay_chain_info(&client_at_block, at).await
            } else {
                Err(CoretimeError::UnsupportedChainType)
            }
        }
    }
}

// ============================================================================
// Coretime Chain Handler (Broker Pallet)
// ============================================================================

async fn handle_coretime_chain_info(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    at: AtResponse,
    block_number: u64,
) -> Result<Response, CoretimeError> {
    // Verify broker pallet exists
    if !has_broker_pallet(client_at_block) {
        return Err(CoretimeError::BrokerPalletNotFound);
    }

    // Fetch all data in parallel
    let (config_result, sale_result, status_result, timeslice_period_result) = tokio::join!(
        fetch_configuration(client_at_block),
        fetch_sale_info(client_at_block),
        fetch_status(client_at_block),
        fetch_timeslice_period(client_at_block)
    );

    let config = config_result?;
    let sale_info = sale_result?;
    let status = status_result?;
    let timeslice_period = timeslice_period_result.unwrap_or(80); // Default to 80 if not found

    // Build response based on available data
    let configuration = config.as_ref().map(|c| ConfigurationInfo {
        region_length: c.region_length,
        interlude_length: c.interlude_length,
        leadin_length: sale_info
            .as_ref()
            .map(|s| s.leadin_length)
            .unwrap_or(c.leadin_length),
        relay_blocks_per_timeslice: timeslice_period,
    });

    let current_region = sale_info.as_ref().and_then(|sale| {
        config.as_ref().map(|cfg| {
            let (start, end) =
                calculate_current_region(sale.region_begin, sale.region_end, cfg.region_length);
            CurrentRegionInfo { start, end }
        })
    });

    let cores = sale_info.as_ref().map(|sale| {
        let current_price =
            calculate_current_core_price(block_number as u32, sale, timeslice_period);
        CoresInfo {
            available: sale.cores_offered.saturating_sub(sale.cores_sold) as u32,
            sold: sale.cores_sold as u32,
            total: sale.cores_offered as u32,
            current_core_price: current_price.to_string(),
            sellout_price: sale.sellout_price.map(|p| p.to_string()),
            first_core: Some(sale.first_core as u32),
        }
    });

    let phase = if let (Some(cfg), Some(sale), Some(st)) = (&config, &sale_info, &status) {
        Some(calculate_phase_config(
            sale.region_begin,
            cfg.region_length,
            cfg.interlude_length,
            sale.leadin_length,
            st.last_committed_timeslice,
            timeslice_period,
        ))
    } else {
        None
    };

    let response = CoretimeInfoResponse {
        at,
        configuration,
        current_region,
        cores,
        phase,
    };

    Ok((StatusCode::OK, Json(response)).into_response())
}

// ============================================================================
// Relay Chain Handler (Coretime Pallet)
// ============================================================================

async fn handle_relay_chain_info(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    at: AtResponse,
) -> Result<Response, CoretimeError> {
    // Verify at least one of the expected pallets exists
    if !has_coretime_pallet(client_at_block) {
        return Err(CoretimeError::CoretimePalletNotFound);
    }

    // Fetch relay chain coretime info
    let (broker_id, pallet_version, max_historical_revenue) = tokio::join!(
        fetch_broker_id(client_at_block),
        fetch_pallet_version(client_at_block),
        fetch_max_historical_revenue(client_at_block)
    );

    let response = CoretimeRelayInfoResponse {
        at,
        broker_id: broker_id.ok().flatten(),
        pallet_version: pallet_version.ok().flatten(),
        max_historical_revenue: max_historical_revenue.ok().flatten(),
    };

    Ok((StatusCode::OK, Json(response)).into_response())
}

// ============================================================================
// Data Fetching - Coretime Chain (Broker Pallet)
// ============================================================================

/// Fetches Configuration from Broker pallet.
async fn fetch_configuration(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Option<ConfigRecord>, CoretimeError> {
    let config_addr = subxt::dynamic::storage::<(), scale_value::Value>("Broker", "Configuration");

    match client_at_block.storage().fetch(config_addr, ()).await {
        Ok(value) => {
            let raw_bytes = value.into_bytes();
            let config = ConfigRecord::decode(&mut &raw_bytes[..]).map_err(|e| {
                CoretimeError::StorageDecodeFailed {
                    pallet: "Broker",
                    entry: "Configuration",
                    details: e.to_string(),
                }
            })?;
            Ok(Some(config))
        }
        Err(subxt::error::StorageError::StorageEntryNotFound { .. }) => Ok(None),
        Err(_) => Ok(None), // Return None for other errors to allow partial responses
    }
}

/// Fetches SaleInfo from Broker pallet.
async fn fetch_sale_info(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Option<SaleInfoRecord>, CoretimeError> {
    let sale_addr = subxt::dynamic::storage::<(), scale_value::Value>("Broker", "SaleInfo");

    match client_at_block.storage().fetch(sale_addr, ()).await {
        Ok(value) => {
            let raw_bytes = value.into_bytes();
            let sale = SaleInfoRecord::decode(&mut &raw_bytes[..]).map_err(|e| {
                CoretimeError::StorageDecodeFailed {
                    pallet: "Broker",
                    entry: "SaleInfo",
                    details: e.to_string(),
                }
            })?;
            Ok(Some(sale))
        }
        Err(subxt::error::StorageError::StorageEntryNotFound { .. }) => Ok(None),
        Err(_) => Ok(None),
    }
}

/// Fetches Status from Broker pallet.
async fn fetch_status(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Option<StatusRecord>, CoretimeError> {
    let status_addr = subxt::dynamic::storage::<(), scale_value::Value>("Broker", "Status");

    match client_at_block.storage().fetch(status_addr, ()).await {
        Ok(value) => {
            let raw_bytes = value.into_bytes();
            let status = StatusRecord::decode(&mut &raw_bytes[..]).map_err(|e| {
                CoretimeError::StorageDecodeFailed {
                    pallet: "Broker",
                    entry: "Status",
                    details: e.to_string(),
                }
            })?;
            Ok(Some(status))
        }
        Err(subxt::error::StorageError::StorageEntryNotFound { .. }) => Ok(None),
        Err(_) => Ok(None),
    }
}

/// Fetches TimeslicePeriod constant from Broker pallet.
async fn fetch_timeslice_period(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<u32, CoretimeError> {
    let metadata = client_at_block.metadata();

    let pallet = metadata
        .pallet_by_name("Broker")
        .ok_or(CoretimeError::BrokerPalletNotFound)?;

    let constant = pallet.constant_by_name("TimeslicePeriod").ok_or(
        CoretimeError::ConstantFetchFailed {
            pallet: "Broker",
            constant: "TimeslicePeriod",
        },
    )?;

    // Decode as u32
    let value = u32::decode(&mut &constant.value()[..]).map_err(|e| {
        CoretimeError::StorageDecodeFailed {
            pallet: "Broker",
            entry: "TimeslicePeriod",
            details: e.to_string(),
        }
    })?;

    Ok(value)
}

// ============================================================================
// Data Fetching - Relay Chain (Coretime Pallet)
// ============================================================================

/// Fetches BrokerId constant from Coretime pallet.
async fn fetch_broker_id(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Option<u32>, CoretimeError> {
    let metadata = client_at_block.metadata();

    let pallet = match metadata.pallet_by_name("Coretime") {
        Some(p) => p,
        None => return Ok(None),
    };

    let constant = match pallet.constant_by_name("BrokerId") {
        Some(c) => c,
        None => return Ok(None),
    };

    let value = u32::decode(&mut &constant.value()[..]).ok();
    Ok(value)
}

/// Fetches pallet version from CoretimeAssignmentProvider.
async fn fetch_pallet_version(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Option<u32>, CoretimeError> {
    // Try to fetch from CoretimeAssignmentProvider::PalletVersion storage
    let version_addr = subxt::dynamic::storage::<(), scale_value::Value>(
        "CoretimeAssignmentProvider",
        "PalletVersion",
    );

    match client_at_block.storage().fetch(version_addr, ()).await {
        Ok(value) => {
            let raw_bytes = value.into_bytes();
            // Pallet version is typically stored as u16
            if let Ok(version) = u16::decode(&mut &raw_bytes[..]) {
                return Ok(Some(version as u32));
            }
            // Try as u32
            if let Ok(version) = u32::decode(&mut &raw_bytes[..]) {
                return Ok(Some(version));
            }
            Ok(None)
        }
        Err(_) => Ok(None),
    }
}

/// Fetches MaxHistoricalRevenue constant from OnDemandAssignmentProvider.
async fn fetch_max_historical_revenue(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Option<u32>, CoretimeError> {
    let metadata = client_at_block.metadata();

    let pallet = match metadata.pallet_by_name("OnDemandAssignmentProvider") {
        Some(p) => p,
        None => return Ok(None),
    };

    let constant = match pallet.constant_by_name("MaxHistoricalRevenue") {
        Some(c) => c,
        None => return Ok(None),
    };

    let value = u32::decode(&mut &constant.value()[..]).ok();
    Ok(value)
}

// ============================================================================
// Calculation Helpers
// ============================================================================

/// Calculates the current region start and end timeslices.
fn calculate_current_region(
    region_begin: u32,
    region_end: u32,
    _region_length: u32,
) -> (Option<u32>, Option<u32>) {
    (Some(region_begin), Some(region_end))
}

/// Calculates the current core price based on the lead-in pricing mechanism.
///
/// The price starts at 2x the end_price and linearly decreases to end_price
/// over the leadin_length period.
fn calculate_current_core_price(
    block_number: u32,
    sale_info: &SaleInfoRecord,
    _timeslice_period: u32,
) -> u128 {
    const SCALE: u128 = 1_000_000_000; // 10^9 for precision

    let sale_start = sale_info.sale_start;
    let leadin_length = sale_info.leadin_length;
    let end_price = sale_info.end_price;

    // Before sale starts, price is 2x end price
    if block_number < sale_start {
        return end_price.saturating_mul(2);
    }

    // After leadin period, price is end_price
    let elapsed = block_number.saturating_sub(sale_start);
    if elapsed >= leadin_length || leadin_length == 0 {
        return end_price;
    }

    // During leadin: linear interpolation from 2x to 1x
    // progress goes from 0 to 10000 (representing 0% to 100%)
    let progress = (elapsed as u128)
        .saturating_mul(10000)
        .checked_div(leadin_length as u128)
        .unwrap_or(10000);

    // leadin_factor goes from 2.0 (at progress=0) to 1.0 (at progress=10000)
    // factor = 2 - (progress / 10000) = (20000 - progress) / 10000
    let factor_scaled = SCALE
        .saturating_mul(2)
        .saturating_sub(progress.saturating_mul(SCALE).checked_div(10000).unwrap_or(0));

    // price = end_price * factor
    factor_scaled
        .saturating_mul(end_price)
        .checked_div(SCALE)
        .unwrap_or(end_price)
}

/// Calculates the phase configuration for the current sale.
fn calculate_phase_config(
    region_begin: u32,
    region_length: u32,
    interlude_length: u32,
    leadin_length: u32,
    last_committed_timeslice: u32,
    timeslice_period: u32,
) -> PhaseInfo {
    // Calculate phase boundaries in timeslices
    // The sale cycle goes: renewals -> priceDiscovery -> fixedPrice
    //
    // renewals: from region_begin - interlude_length/timeslice_period to region_begin
    // priceDiscovery: from region_begin to region_begin + leadin_length/timeslice_period
    // fixedPrice: rest of the region

    let interlude_timeslices = interlude_length
        .checked_div(timeslice_period)
        .unwrap_or(0);
    let leadin_timeslices = leadin_length.checked_div(timeslice_period).unwrap_or(0);

    let renewals_end = region_begin;
    let price_discovery_end = renewals_end.saturating_add(leadin_timeslices);
    let fixed_price_end = region_begin.saturating_add(region_length);

    // Determine current phase based on last_committed_timeslice
    let current_phase = if last_committed_timeslice < renewals_end.saturating_sub(interlude_timeslices)
    {
        "renewals"
    } else if last_committed_timeslice < price_discovery_end {
        "priceDiscovery"
    } else {
        "fixedPrice"
    };

    PhaseInfo {
        current_phase: current_phase.to_string(),
        config: vec![
            PhaseConfig {
                phase_name: "renewals".to_string(),
                last_relay_block: renewals_end.saturating_mul(timeslice_period),
                last_timeslice: renewals_end,
            },
            PhaseConfig {
                phase_name: "priceDiscovery".to_string(),
                last_relay_block: price_discovery_end.saturating_mul(timeslice_period),
                last_timeslice: price_discovery_end,
            },
            PhaseConfig {
                phase_name: "fixedPrice".to_string(),
                last_relay_block: fixed_price_end.saturating_mul(timeslice_period),
                last_timeslice: fixed_price_end,
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_price_before_sale_start() {
        let sale_info = SaleInfoRecord {
            sale_start: 1000,
            leadin_length: 500,
            end_price: 1_000_000_000_000, // 1 DOT
            region_begin: 100,
            region_end: 200,
            ideal_cores_sold: 10,
            cores_offered: 20,
            first_core: 0,
            sellout_price: None,
            cores_sold: 5,
        };

        // Before sale starts, price should be 2x end price
        let price = calculate_current_core_price(500, &sale_info, 80);
        assert_eq!(price, 2_000_000_000_000);
    }

    #[test]
    fn test_price_at_sale_start() {
        let sale_info = SaleInfoRecord {
            sale_start: 1000,
            leadin_length: 500,
            end_price: 1_000_000_000_000,
            region_begin: 100,
            region_end: 200,
            ideal_cores_sold: 10,
            cores_offered: 20,
            first_core: 0,
            sellout_price: None,
            cores_sold: 5,
        };

        // At sale start (elapsed=0), price should be 2x
        let price = calculate_current_core_price(1000, &sale_info, 80);
        assert_eq!(price, 2_000_000_000_000);
    }

    #[test]
    fn test_price_after_leadin() {
        let sale_info = SaleInfoRecord {
            sale_start: 1000,
            leadin_length: 500,
            end_price: 1_000_000_000_000,
            region_begin: 100,
            region_end: 200,
            ideal_cores_sold: 10,
            cores_offered: 20,
            first_core: 0,
            sellout_price: None,
            cores_sold: 5,
        };

        // After leadin period, price should be end_price
        let price = calculate_current_core_price(1600, &sale_info, 80);
        assert_eq!(price, 1_000_000_000_000);
    }

    #[test]
    fn test_price_midway_through_leadin() {
        let sale_info = SaleInfoRecord {
            sale_start: 1000,
            leadin_length: 1000,
            end_price: 1_000_000_000_000,
            region_begin: 100,
            region_end: 200,
            ideal_cores_sold: 10,
            cores_offered: 20,
            first_core: 0,
            sellout_price: None,
            cores_sold: 5,
        };

        // Midway (50% through leadin), price should be 1.5x end_price
        let price = calculate_current_core_price(1500, &sale_info, 80);
        assert_eq!(price, 1_500_000_000_000);
    }

    #[test]
    fn test_phase_config_structure() {
        let phase = calculate_phase_config(
            100,  // region_begin
            1000, // region_length
            800,  // interlude_length
            400,  // leadin_length
            50,   // last_committed_timeslice
            80,   // timeslice_period
        );

        assert_eq!(phase.config.len(), 3);
        assert_eq!(phase.config[0].phase_name, "renewals");
        assert_eq!(phase.config[1].phase_name, "priceDiscovery");
        assert_eq!(phase.config[2].phase_name, "fixedPrice");
    }

    #[test]
    fn test_current_phase_renewals() {
        let phase = calculate_phase_config(
            100,  // region_begin
            1000, // region_length
            800,  // interlude_length (10 timeslices)
            400,  // leadin_length (5 timeslices)
            80,   // last_committed_timeslice (before renewals_end - interlude)
            80,   // timeslice_period
        );

        assert_eq!(phase.current_phase, "renewals");
    }

    #[test]
    fn test_current_phase_price_discovery() {
        let phase = calculate_phase_config(
            100,  // region_begin
            1000, // region_length
            800,  // interlude_length
            400,  // leadin_length (5 timeslices)
            102,  // last_committed_timeslice (between region_begin and leadin end)
            80,   // timeslice_period
        );

        assert_eq!(phase.current_phase, "priceDiscovery");
    }

    #[test]
    fn test_current_phase_fixed_price() {
        let phase = calculate_phase_config(
            100,  // region_begin
            1000, // region_length
            800,  // interlude_length
            400,  // leadin_length (5 timeslices)
            200,  // last_committed_timeslice (after leadin)
            80,   // timeslice_period
        );

        assert_eq!(phase.current_phase, "fixedPrice");
    }

    #[test]
    fn test_coretime_info_response_serialization() {
        let response = CoretimeInfoResponse {
            at: AtResponse {
                hash: "0xabc123".to_string(),
                height: "12345".to_string(),
            },
            configuration: Some(ConfigurationInfo {
                region_length: 5040,
                interlude_length: 50400,
                leadin_length: 100800,
                relay_blocks_per_timeslice: 80,
            }),
            current_region: Some(CurrentRegionInfo {
                start: Some(100),
                end: Some(200),
            }),
            cores: Some(CoresInfo {
                available: 15,
                sold: 5,
                total: 20,
                current_core_price: "1000000000000".to_string(),
                sellout_price: None,
                first_core: Some(43),
            }),
            phase: Some(PhaseInfo {
                current_phase: "priceDiscovery".to_string(),
                config: vec![],
            }),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"at\""));
        assert!(json.contains("\"configuration\""));
        assert!(json.contains("\"regionLength\":5040"));
        assert!(json.contains("\"cores\""));
        assert!(json.contains("\"available\":15"));
    }

    #[test]
    fn test_relay_info_response_serialization() {
        let response = CoretimeRelayInfoResponse {
            at: AtResponse {
                hash: "0xabc123".to_string(),
                height: "12345".to_string(),
            },
            broker_id: Some(1005),
            pallet_version: Some(1),
            max_historical_revenue: Some(28800),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"brokerId\":1005"));
        assert!(json.contains("\"palletVersion\":1"));
        assert!(json.contains("\"maxHistoricalRevenue\":28800"));
    }

    #[test]
    fn test_relay_info_response_skips_none() {
        let response = CoretimeRelayInfoResponse {
            at: AtResponse {
                hash: "0xabc123".to_string(),
                height: "12345".to_string(),
            },
            broker_id: None,
            pallet_version: None,
            max_historical_revenue: None,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(!json.contains("brokerId"));
        assert!(!json.contains("palletVersion"));
        assert!(!json.contains("maxHistoricalRevenue"));
    }

    #[test]
    fn test_current_region_calculation() {
        let (start, end) = calculate_current_region(100, 200, 100);
        assert_eq!(start, Some(100));
        assert_eq!(end, Some(200));
    }

    #[test]
    fn test_cores_info_serialization() {
        let cores = CoresInfo {
            available: 10,
            sold: 5,
            total: 15,
            current_core_price: "1000000000000".to_string(),
            sellout_price: Some("500000000000".to_string()),
            first_core: Some(43),
        };

        let json = serde_json::to_string(&cores).unwrap();
        assert!(json.contains("\"available\":10"));
        assert!(json.contains("\"sold\":5"));
        assert!(json.contains("\"total\":15"));
        assert!(json.contains("\"currentCorePrice\":\"1000000000000\""));
        assert!(json.contains("\"selloutPrice\":\"500000000000\""));
        assert!(json.contains("\"firstCore\":43"));
    }

    #[test]
    fn test_cores_info_serialization_without_optional_fields() {
        let cores = CoresInfo {
            available: 10,
            sold: 5,
            total: 15,
            current_core_price: "1000000000000".to_string(),
            sellout_price: None,
            first_core: None,
        };

        let json = serde_json::to_string(&cores).unwrap();
        assert!(!json.contains("selloutPrice"));
        assert!(!json.contains("firstCore"));
    }
}
