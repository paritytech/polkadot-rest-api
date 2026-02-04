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
use primitive_types::H256;
use scale_decode::DecodeAsType;
use serde::Serialize;
use std::str::FromStr;
use subxt::{SubstrateConfig, client::OnlineClientAtBlock};

const SCALE: u32 = 10000;

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
    pub region_length: String,
    /// Length of the interlude period in relay blocks.
    pub interlude_length: String,
    /// Length of the leadin period in relay blocks.
    pub leadin_length: String,
    /// Number of relay chain blocks per timeslice.
    pub relay_blocks_per_timeslice: String,
}

/// Broker status information.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusInfo {
    /// Total number of cores.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub core_count: Option<u32>,
    /// Number of cores in the private pool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_pool_size: Option<u32>,
    /// Number of cores in the system pool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_pool_size: Option<u32>,
    /// Last committed timeslice.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_committed_timeslice: Option<u32>,
    /// Last timeslice.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_timeslice: Option<u32>,
}

/// Current region timing information.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentRegionInfo {
    /// Start timeslice of the current region.
    pub start: Option<String>,
    /// End timeslice of the current region.
    pub end: Option<String>,
}

/// Core availability and pricing information.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoresInfo {
    /// Number of cores available for purchase.
    pub available: String,
    /// Number of cores already sold.
    pub sold: String,
    /// Total cores offered for sale.
    pub total: String,
    /// Current price per core.
    pub current_core_price: String,
    /// Price at which cores sold out (if applicable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sellout_price: Option<String>,
    /// First core index in the sale.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_core: Option<String>,
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
    pub last_relay_block: String,
    /// Last timeslice of this phase.
    pub last_timeslice: String,
}

/// Response for GET /coretime/info endpoint on relay chains.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoretimeRelayInfoResponse {
    /// Block context (hash and height).
    pub at: AtResponse,
    /// Parachain ID of the coretime broker chain.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broker_id: Option<String>,
    /// Pallet version.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pallet_version: Option<String>,
    /// Maximum historical revenue blocks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_historical_revenue: Option<String>,
}

/// Derives DecodeAsType for subxt dynamic storage compatibility.
#[derive(Debug, Clone, Default, DecodeAsType)]
struct ConfigRecord {
    interlude_length: u32,
    leadin_length: u32,
    region_length: u32,
}

/// Derives DecodeAsType for subxt dynamic storage compatibility.
#[derive(Debug, Clone, Default, DecodeAsType)]
struct SaleInfoRecord {
    sale_start: u32,
    leadin_length: u32,
    end_price: u128,
    region_begin: u32,
    region_end: u32,
    cores_offered: u16,
    first_core: u16,
    sellout_price: Option<u128>,
    cores_sold: u16,
}

/// StatusRecord from Broker pallet - decoded directly via SCALE.
/// Derives DecodeAsType for subxt dynamic storage compatibility.
#[derive(Debug, Clone, Default, DecodeAsType)]
struct StatusRecord {
    last_committed_timeslice: u32,
}

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

async fn handle_coretime_chain_info(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
    at: AtResponse,
    block_number: u64,
) -> Result<Response, CoretimeError> {
    // Verify broker pallet exists
    if !has_broker_pallet(client_at_block) {
        return Err(CoretimeError::BrokerPalletNotFound);
    }

    // Fetch all data in parallel using subxt's decode_as for type-safe decoding
    let (config_result, sale_result, status_result, timeslice_period_result, relay_block_result) = tokio::join!(
        fetch_configuration(client_at_block),
        fetch_sale_info(client_at_block),
        fetch_status(client_at_block),
        fetch_timeslice_period(client_at_block),
        fetch_relay_block_number(client_at_block)
    );

    let config = config_result?;
    let sale_info = sale_result?;
    let status = status_result?;
    let timeslice_period = timeslice_period_result.unwrap_or(80); // Default to 80 if not found
    // Use relay chain block number for price calculation since sale_start/leadin_length
    // are stored as relay block numbers. Fall back to parachain block number if unavailable.
    let price_block_number = relay_block_result
        .unwrap_or(None)
        .unwrap_or(block_number as u32);

    // Build response based on available data
    let configuration = config.as_ref().map(|c| ConfigurationInfo {
        region_length: c.region_length.to_string(),
        interlude_length: c.interlude_length.to_string(),
        leadin_length: sale_info
            .as_ref()
            .map(|s| s.leadin_length)
            .unwrap_or(c.leadin_length)
            .to_string(),
        relay_blocks_per_timeslice: timeslice_period.to_string(),
    });

    let current_region = sale_info.as_ref().map(|sale| CurrentRegionInfo {
        start: Some(sale.region_begin.to_string()),
        end: Some(sale.region_end.to_string()),
    });

    let cores = sale_info.as_ref().map(|sale| {
        let current_price = calculate_current_core_price(price_block_number, sale);
        CoresInfo {
            available: (sale.cores_offered as u32)
                .saturating_sub(sale.cores_sold as u32)
                .to_string(),
            sold: (sale.cores_sold as u32).to_string(),
            total: (sale.cores_offered as u32).to_string(),
            current_core_price: current_price.to_string(),
            sellout_price: sale.sellout_price.map(|p| p.to_string()),
            first_core: Some((sale.first_core as u32).to_string()),
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
        fetch_pallet_version_decoded(client_at_block),
        fetch_max_historical_revenue(client_at_block)
    );

    let response = CoretimeRelayInfoResponse {
        at,
        broker_id: broker_id.ok().flatten().map(|v| v.to_string()),
        pallet_version: pallet_version.ok().flatten().map(|v| v.to_string()),
        max_historical_revenue: max_historical_revenue.ok().flatten().map(|v| v.to_string()),
    };

    Ok((StatusCode::OK, Json(response)).into_response())
}

/// Fetches and decodes Configuration from Broker pallet directly into ConfigRecord.
/// Uses subxt dynamic storage with DecodeAsType for type-safe decoding.
async fn fetch_configuration(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Option<ConfigRecord>, CoretimeError> {
    let config_addr = subxt::dynamic::storage::<(), ConfigRecord>("Broker", "Configuration");

    match client_at_block.storage().fetch(config_addr, ()).await {
        Ok(storage_value) => {
            let decoded =
                storage_value
                    .decode()
                    .map_err(|e| CoretimeError::StorageDecodeFailed {
                        pallet: "Broker",
                        entry: "Configuration",
                        details: e.to_string(),
                    })?;
            Ok(Some(decoded))
        }
        Err(subxt::error::StorageError::StorageEntryNotFound { .. }) => {
            tracing::debug!("Could not find Broker.Configuration storage entry.");
            Ok(None)
        }
        Err(e) => {
            tracing::debug!(
                "Failed to retrieve Broker.Configuration: {:?}",
                format!("{e}")
            );
            Ok(None)
        }
    }
}

/// Fetches and decodes SaleInfo from Broker pallet directly into SaleInfoRecord.
/// Uses subxt dynamic storage with DecodeAsType for type-safe decoding.
async fn fetch_sale_info(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Option<SaleInfoRecord>, CoretimeError> {
    let sale_addr = subxt::dynamic::storage::<(), SaleInfoRecord>("Broker", "SaleInfo");

    match client_at_block.storage().fetch(sale_addr, ()).await {
        Ok(storage_value) => {
            let decoded =
                storage_value
                    .decode()
                    .map_err(|e| CoretimeError::StorageDecodeFailed {
                        pallet: "Broker",
                        entry: "SaleInfo",
                        details: e.to_string(),
                    })?;
            Ok(Some(decoded))
        }
        Err(subxt::error::StorageError::StorageEntryNotFound { .. }) => {
            tracing::debug!("Could not find Broker.SaleInfo storage entry.");
            Ok(None)
        }
        Err(e) => {
            tracing::debug!("Failed to retrieve Broker.SaleInfo: {:?}", format!("{e}"));
            Ok(None)
        }
    }
}

/// Fetches and decodes Status from Broker pallet directly into StatusRecord.
/// Uses subxt dynamic storage with DecodeAsType for type-safe decoding.
async fn fetch_status(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Option<StatusRecord>, CoretimeError> {
    let status_addr = subxt::dynamic::storage::<(), StatusRecord>("Broker", "Status");

    match client_at_block.storage().fetch(status_addr, ()).await {
        Ok(storage_value) => {
            let decoded =
                storage_value
                    .decode()
                    .map_err(|e| CoretimeError::StorageDecodeFailed {
                        pallet: "Broker",
                        entry: "Status",
                        details: e.to_string(),
                    })?;
            Ok(Some(decoded))
        }
        Err(subxt::error::StorageError::StorageEntryNotFound { .. }) => {
            tracing::debug!("Could not find Broker.Status storage entry.");
            Ok(None)
        }
        Err(e) => {
            tracing::debug!("Failed to retrieve Broker.Status: {:?}", format!("{e}"));
            Ok(None)
        }
    }
}

/// Fetches the relay chain block number from ParachainSystem pallet.
/// On coretime parachains, sale_start and leadin_length are stored as relay chain
/// block numbers, so we need the relay block number for accurate price calculation.
async fn fetch_relay_block_number(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Option<u32>, CoretimeError> {
    let addr = subxt::dynamic::storage::<(), u32>("ParachainSystem", "LastRelayChainBlockNumber");

    match client_at_block.storage().fetch(addr, ()).await {
        Ok(storage_value) => {
            let decoded =
                storage_value
                    .decode()
                    .map_err(|e| CoretimeError::StorageDecodeFailed {
                        pallet: "ParachainSystem",
                        entry: "LastRelayChainBlockNumber",
                        details: e.to_string(),
                    })?;
            Ok(Some(decoded))
        }
        Err(e) => {
            tracing::debug!(
                "Failed to retrieve ParachainSystem.LastRelayChainBlockNumber: {:?}",
                format!("{e}")
            );
            Ok(None)
        }
    }
}

/// Fetches TimeslicePeriod constant from Broker pallet.
async fn fetch_timeslice_period(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<u32, CoretimeError> {
    let addr = subxt::dynamic::constant::<u32>("Broker", "TimeslicePeriod");
    let value = client_at_block.constants().entry(addr).map_err(|_| {
        CoretimeError::ConstantFetchFailed {
            pallet: "Broker",
            constant: "TimeslicePeriod",
        }
    })?;

    Ok(value)
}

/// Fetches BrokerId constant from Coretime pallet.
async fn fetch_broker_id(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Option<u32>, CoretimeError> {
    let addr = subxt::dynamic::constant::<u32>("Coretime", "BrokerId");
    let value = client_at_block.constants().entry(addr).map_err(|_| {
        CoretimeError::ConstantFetchFailed {
            pallet: "Coretime",
            constant: "BrokerId",
        }
    })?;
    Ok(Some(value))
}

/// Fetches pallet version from CoretimeAssignmentProvider using scale_value.
async fn fetch_pallet_version_decoded(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Option<u16>, CoretimeError> {
    // Try to fetch from CoretimeAssignmentProvider::PalletVersion storage
    let version_addr =
        subxt::dynamic::storage::<(), u16>("CoretimeAssignmentProvider", "PalletVersion");
    let version = client_at_block
        .storage()
        .fetch(version_addr, ())
        .await
        .map_err(|_| CoretimeError::StorageFetchFailed {
            pallet: "CoretimeAssignmentProvider",
            entry: "PalletVersion",
        })?;

    let decoded = version
        .decode()
        .map_err(|e| CoretimeError::StorageDecodeFailed {
            pallet: "CoretimeAssignmentProvider",
            entry: "PalletVersion",
            details: e.to_string(),
        })?;

    Ok(Some(decoded))
}

/// Fetches MaxHistoricalRevenue constant from OnDemandAssignmentProvider.
async fn fetch_max_historical_revenue(
    client_at_block: &OnlineClientAtBlock<SubstrateConfig>,
) -> Result<Option<u32>, CoretimeError> {
    let addr = subxt::dynamic::constant::<u32>("OnDemand", "MaxHistoricalRevenue");
    if let Ok(value) = client_at_block.constants().entry(addr) {
        Ok(Some(value))
    } else {
        let legacy_addr =
            subxt::dynamic::constant::<u32>("OnDemandAssignmentProvider", "MaxHistoricalRevenue");
        let value = client_at_block
            .constants()
            .entry(legacy_addr)
            .map_err(|_| CoretimeError::ConstantFetchFailed {
                pallet: "OnDemandAssignmentProvider",
                constant: "MaxHistoricalRevenue",
            })?;

        Ok(Some(value))
    }
}

// ============================================================================
// Value Extraction Helpers
// ============================================================================
fn calculate_leading_at(scaled_when: u32) -> u32 {
    let scaled_half = SCALE.saturating_div(2);

    if scaled_when.lt(&scaled_half) || scaled_when.eq(&scaled_half) {
        SCALE
            .saturating_mul(100)
            .saturating_sub(scaled_when.saturating_mul(180))
    } else {
        SCALE
            .saturating_mul(19)
            .saturating_sub(scaled_when.saturating_mul(18))
    }
}
fn calculate_current_core_price(block_number: u32, sale_info: &SaleInfoRecord) -> u128 {
    let sale_start = sale_info.sale_start;
    let leadin_length = sale_info.leadin_length;
    let end_price = sale_info.end_price;

    let elapsed_time_since_sale_start = block_number.saturating_sub(sale_start);
    let capped_elapsed_time = match elapsed_time_since_sale_start.lt(&leadin_length) {
        true => elapsed_time_since_sale_start,
        false => leadin_length,
    };

    let scaled_progress: u32 = capped_elapsed_time
        .saturating_mul(SCALE)
        .saturating_div(leadin_length);

    let leadin_factor: u128 = calculate_leading_at(scaled_progress).into();

    leadin_factor
        .saturating_mul(end_price)
        .saturating_div(u128::from(SCALE))
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

    let interlude_timeslices = interlude_length.checked_div(timeslice_period).unwrap_or(0);
    let leadin_timeslices = leadin_length.checked_div(timeslice_period).unwrap_or(0);

    let renewals_end = region_begin;
    let price_discovery_end = renewals_end.saturating_add(leadin_timeslices);
    let fixed_price_end = region_begin.saturating_add(region_length);

    // Determine current phase based on last_committed_timeslice
    let current_phase =
        if last_committed_timeslice < renewals_end.saturating_sub(interlude_timeslices) {
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
                last_relay_block: renewals_end.saturating_mul(timeslice_period).to_string(),
                last_timeslice: renewals_end.to_string(),
            },
            PhaseConfig {
                phase_name: "priceDiscovery".to_string(),
                last_relay_block: price_discovery_end
                    .saturating_mul(timeslice_period)
                    .to_string(),
                last_timeslice: price_discovery_end.to_string(),
            },
            PhaseConfig {
                phase_name: "fixedPrice".to_string(),
                last_relay_block: fixed_price_end.saturating_mul(timeslice_period).to_string(),
                last_timeslice: fixed_price_end.to_string(),
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
            cores_offered: 20,
            cores_sold: 5,
            first_core: 0,
            sellout_price: None,
        };

        // Before sale starts (elapsed=0), price should be 100x end price (max leadin factor)
        let price = calculate_current_core_price(500, &sale_info);
        assert_eq!(price, 100_000_000_000_000);
    }

    #[test]
    fn test_price_at_sale_start() {
        let sale_info = SaleInfoRecord {
            sale_start: 1000,
            leadin_length: 500,
            end_price: 1_000_000_000_000,
            region_begin: 100,
            region_end: 200,
            cores_offered: 20,
            cores_sold: 5,
            first_core: 0,
            sellout_price: None,
        };

        // At sale start (elapsed=0), price should be 100x end price (max leadin factor)
        let price = calculate_current_core_price(1000, &sale_info);
        assert_eq!(price, 100_000_000_000_000);
    }

    #[test]
    fn test_price_after_leadin() {
        let sale_info = SaleInfoRecord {
            sale_start: 1000,
            leadin_length: 500,
            end_price: 1_000_000_000_000,
            region_begin: 100,
            region_end: 200,
            cores_offered: 20,
            cores_sold: 5,
            first_core: 0,
            sellout_price: None,
        };

        // After leadin period, price should be end_price
        let price = calculate_current_core_price(1600, &sale_info);
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
            cores_offered: 20,
            cores_sold: 5,
            first_core: 0,
            sellout_price: None,
        };

        // At 50% through leadin, CenterTargetPrice factor is 10x end_price
        let price = calculate_current_core_price(1500, &sale_info);
        assert_eq!(price, 10_000_000_000_000);
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
                region_length: "5040".to_string(),
                interlude_length: "50400".to_string(),
                leadin_length: "100800".to_string(),
                relay_blocks_per_timeslice: "80".to_string(),
            }),
            current_region: Some(CurrentRegionInfo {
                start: Some("100".to_string()),
                end: Some("200".to_string()),
            }),
            cores: Some(CoresInfo {
                available: "15".to_string(),
                sold: "5".to_string(),
                total: "20".to_string(),
                current_core_price: "1000000000000".to_string(),
                sellout_price: None,
                first_core: Some("43".to_string()),
            }),
            phase: Some(PhaseInfo {
                current_phase: "priceDiscovery".to_string(),
                config: vec![],
            }),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"at\""));
        assert!(json.contains("\"configuration\""));
        assert!(json.contains("\"regionLength\":\"5040\""));
        assert!(json.contains("\"cores\""));
        assert!(json.contains("\"available\":\"15\""));
        assert!(json.contains("\"currentCorePrice\":\"1000000000000\""));
        assert!(json.contains("\"phase\""));
        assert!(json.contains("\"currentPhase\":\"priceDiscovery\""));
    }

    #[test]
    fn test_relay_info_response_serialization() {
        let response = CoretimeRelayInfoResponse {
            at: AtResponse {
                hash: "0xabc123".to_string(),
                height: "12345".to_string(),
            },
            broker_id: Some("1005".to_string()),
            pallet_version: Some("1".to_string()),
            max_historical_revenue: Some("28800".to_string()),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"brokerId\":\"1005\""));
        assert!(json.contains("\"palletVersion\":\"1\""));
        assert!(json.contains("\"maxHistoricalRevenue\":\"28800\""));
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
    fn test_cores_info_serialization() {
        let cores = CoresInfo {
            available: "10".to_string(),
            sold: "5".to_string(),
            total: "15".to_string(),
            current_core_price: "1000000000000".to_string(),
            sellout_price: Some("500000000000".to_string()),
            first_core: Some("43".to_string()),
        };

        let json = serde_json::to_string(&cores).unwrap();
        assert!(json.contains("\"available\":\"10\""));
        assert!(json.contains("\"sold\":\"5\""));
        assert!(json.contains("\"total\":\"15\""));
        assert!(json.contains("\"currentCorePrice\":\"1000000000000\""));
        assert!(json.contains("\"selloutPrice\":\"500000000000\""));
        assert!(json.contains("\"firstCore\":\"43\""));
    }

    #[test]
    fn test_cores_info_serialization_without_optional_fields() {
        let cores = CoresInfo {
            available: "10".to_string(),
            sold: "5".to_string(),
            total: "15".to_string(),
            current_core_price: "1000000000000".to_string(),
            sellout_price: None,
            first_core: None,
        };

        let json = serde_json::to_string(&cores).unwrap();
        assert!(!json.contains("selloutPrice"));
        assert!(!json.contains("firstCore"));
    }
}
