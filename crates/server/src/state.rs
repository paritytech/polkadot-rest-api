// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::routes::RouteRegistry;
use crate::utils::QueryFeeDetailsCache;
use polkadot_rest_api_config::{ChainType, SidecarConfig};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use subxt::config::RpcConfigFor;
use subxt::{OnlineClient, SubstrateConfig};
use subxt_rpcs::client::reconnecting_rpc_client::{
    ExponentialBackoff, RpcClient as ReconnectingRpcClient,
};
use subxt_rpcs::{LegacyRpcMethods, RpcClient, rpc_params};
use tokio::sync::OnceCell;

/// Type alias for LegacyRpcMethods with correct RpcConfig wrapper
pub type SubstrateLegacyRpc = LegacyRpcMethods<RpcConfigFor<SubstrateConfig>>;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StateError {
    #[error("Failed to load configuration")]
    ConfigLoadFailed(#[from] polkadot_rest_api_config::ConfigError),

    #[error("Failed to connect to substrate node at {url}")]
    ConnectionFailed {
        url: String,
        #[source]
        source: subxt_rpcs::Error,
    },

    #[error("Connection to substrate node at {url} timed out after {timeout_secs} seconds")]
    ConnectionTimeout { url: String, timeout_secs: u64 },

    #[error("Failed to get runtime version")]
    RuntimeVersionFailed(#[source] subxt_rpcs::Error),

    #[error("spec_name not found in runtime version")]
    SpecNameNotFound,
}

/// Error type for relay chain connection operations
#[derive(Debug, Clone, Error)]
pub enum RelayChainError {
    #[error(
        "Relay chain URL not configured. Add a relay chain URL to SAS_SUBSTRATE_MULTI_CHAIN_URL"
    )]
    NotConfigured,

    #[error("Failed to connect to relay chain: {0}")]
    ConnectionFailed(String),
}

/// Information about the connected chain
#[derive(Clone, Debug)]
pub struct ChainInfo {
    /// Type of chain (Relay, AssetHub, Parachain)
    pub chain_type: ChainType,
    /// Runtime spec name (e.g., "polkadot", "asset-hub-polkadot")
    pub spec_name: String,
    /// Current runtime spec version
    pub spec_version: u32,
    /// SS58 address format prefix for this chain
    pub ss58_prefix: u16,
}

#[derive(Clone)]
pub struct AppState {
    pub config: SidecarConfig,
    #[allow(dead_code)] // Will be used when implementing endpoints
    pub client: Arc<OnlineClient<SubstrateConfig>>,
    #[allow(dead_code)] // Will be used when implementing endpoints
    pub legacy_rpc: Arc<SubstrateLegacyRpc>,
    pub rpc_client: Arc<RpcClient>,
    pub chain_info: ChainInfo,

    /// Optional relay chain connection (for parachains)
    #[allow(dead_code)] // Will be used when implementing relay chain endpoints
    pub relay_client: Option<Arc<OnlineClient<SubstrateConfig>>>,
    #[allow(dead_code)] // Will be used when implementing relay chain endpoints
    pub relay_rpc_client: Option<Arc<RpcClient>>,
    #[allow(dead_code)] // Will be used when implementing relay chain endpoints
    pub relay_chain_info: Option<ChainInfo>,

    /// Cache for tracking queryFeeDetails availability per spec version
    pub fee_details_cache: Arc<QueryFeeDetailsCache>,
    /// All chain configurations loaded from chain_config.json
    pub chain_configs: Arc<polkadot_rest_api_config::ChainConfigs>,
    /// Complete configuration with optional relay chain
    pub chain_config: Arc<polkadot_rest_api_config::Config>,
    /// Registry of all available routes for introspection
    pub route_registry: RouteRegistry,
    /// Relay Chain RPC client (only present when multi-chain is configured with a relay chain)
    pub relay_chain_rpc: Option<Arc<SubstrateLegacyRpc>>,
    /// Lazy-initialized relay chain RPC client for when startup connection failed but URL is configured
    pub lazy_relay_rpc: Arc<OnceCell<Arc<RpcClient>>>,
}

impl AppState {
    pub async fn new() -> Result<Self, StateError> {
        let config = SidecarConfig::from_env()?;
        Self::new_with_config(config).await
    }

    pub async fn new_with_config(config: SidecarConfig) -> Result<Self, StateError> {
        let reconnecting_client =
            connect_with_progress_logging(&config.substrate.url, &config).await?;

        // Wrap in RpcClient for compatibility with existing code
        let rpc_client = RpcClient::new(reconnecting_client);

        let legacy_rpc: SubstrateLegacyRpc = LegacyRpcMethods::new(rpc_client.clone());

        // Get chain info first to determine which configuration to load
        let chain_info = get_chain_info(&legacy_rpc).await?;

        // Load all chain configurations
        let chain_configs = Arc::new(polkadot_rest_api_config::ChainConfigs::default());

        // Get configuration for the connected chain (or use defaults)
        let chain_chain_config = chain_configs
            .get(&chain_info.spec_name)
            .cloned()
            .unwrap_or_default();

        // Configure SubstrateConfig with appropriate legacy types based on chain config
        let subxt_config = match chain_chain_config.legacy_types.as_str() {
            "polkadot" => {
                // Load Polkadot-specific legacy types for historic block support
                // Must use builder pattern - SubstrateConfig::new() doesn't have set_legacy_types
                SubstrateConfig::builder()
                    .set_legacy_types(frame_decode::legacy_types::polkadot::relay_chain())
                    .build()
            }
            "kusama-relay" => {
                // Load Kusama relay chain legacy types for historic block support
                SubstrateConfig::builder()
                    .set_legacy_types(frame_decode::legacy_types::kusama::relay_chain())
                    .build()
            }
            "kusama-asset-hub" => {
                // Load Kusama Asset Hub legacy types for historic block support
                SubstrateConfig::builder()
                    .set_legacy_types(frame_decode::legacy_types::kusama::asset_hub())
                    .build()
            }
            _ => {
                // For chains without legacy types or unknown chains, use empty config
                SubstrateConfig::new()
            }
        };

        // Note: from_rpc_client_with_config is now async in the new subxt
        let client = OnlineClient::from_rpc_client_with_config(subxt_config, rpc_client.clone())
            .await
            .map_err(|e| StateError::ConnectionFailed {
                url: config.substrate.url.clone(),
                source: subxt_rpcs::Error::Client(Box::new(std::io::Error::other(e.to_string()))),
            })?;

        // Check if this chain requires a relay chain connection
        let (relay_client, relay_rpc_client, relay_chain_info, relay_chain_config) = if let Some(
            relay_chain_name,
        ) =
            &chain_chain_config.relay_chain
        {
            // If relay chain URL is provided in multi_chain_urls, connect to it
            if let Some(relay_url) = config.substrate.get_relay_chain_url() {
                // Relay chain connection now blocks and fails startup if it cannot connect
                let (client, rpc, info, rc_config) =
                    Self::connect_relay_chain(relay_url, relay_chain_name, &chain_configs, &config)
                        .await?;
                (Some(client), Some(rpc), Some(info), Some(rc_config))
            } else {
                tracing::info!(
                    "Chain '{}' is a parachain with relay chain '{}', but no relay chain URL found in SAS_SUBSTRATE_MULTI_CHAIN_URL. \
                        Relay chain features will be unavailable. Add: '[{{\"url\":\"wss://...\",\"type\":\"relay\"}}]'",
                    chain_info.spec_name,
                    relay_chain_name
                );
                (None, None, None, None)
            }
        } else {
            // Not a parachain or relay chain connection not needed
            (None, None, None, None)
        };

        // Create Config struct with chain and optional relay chain
        let full_config = if let Some(rc_config) = relay_chain_config {
            Arc::new(polkadot_rest_api_config::Config::with_relay_chain(
                chain_chain_config,
                rc_config,
            ))
        } else {
            Arc::new(polkadot_rest_api_config::Config::single_chain(
                chain_chain_config,
            ))
        };

        let relay_chain_rpc = relay_rpc_client
            .as_ref()
            .map(|rpc_client| Arc::new(LegacyRpcMethods::new((**rpc_client).clone())));

        Ok(Self {
            config,
            client: Arc::new(client),
            legacy_rpc: Arc::new(legacy_rpc),
            rpc_client: Arc::new(rpc_client),
            chain_info,
            relay_client,
            relay_rpc_client,
            relay_chain_info,
            fee_details_cache: Arc::new(QueryFeeDetailsCache::new()),
            chain_configs,
            chain_config: full_config,
            route_registry: RouteRegistry::new(),
            relay_chain_rpc,
            lazy_relay_rpc: Arc::new(OnceCell::new()),
        })
    }

    pub fn get_relay_chain_client(&self) -> Option<&Arc<OnlineClient<SubstrateConfig>>> {
        self.relay_client.as_ref()
    }

    pub fn get_relay_chain_rpc(&self) -> Option<&Arc<SubstrateLegacyRpc>> {
        self.relay_chain_rpc.as_ref()
    }

    pub fn get_relay_chain_rpc_client(&self) -> Option<&Arc<RpcClient>> {
        self.relay_rpc_client.as_ref()
    }

    /// Get or lazily initialize the relay chain RPC client.
    ///
    /// This method first checks if a relay chain connection was established at startup.
    /// If not, it attempts to create one lazily using the configured relay chain URL.
    /// The connection is cached after the first successful initialization.
    ///
    /// Note: With the current startup behavior, this lazy path is only hit when:
    /// - Primary chain doesn't require relay chain (no `relay_chain` in chain config)
    /// - But a relay URL was still provided in SAS_SUBSTRATE_MULTI_CHAIN_URL
    pub async fn get_or_init_relay_rpc_client(&self) -> Result<Arc<RpcClient>, RelayChainError> {
        // Return existing connection if available
        if let Some(client) = &self.relay_rpc_client {
            return Ok(client.clone());
        }

        // Try lazy initialization with reconnection support
        self.lazy_relay_rpc
            .get_or_try_init(|| async {
                let relay_url = self
                    .config
                    .substrate
                    .multi_chain_urls
                    .iter()
                    .find(|chain_url| chain_url.chain_type == ChainType::Relay)
                    .map(|chain_url| chain_url.url.clone())
                    .ok_or(RelayChainError::NotConfigured)?;

                // Use reconnecting client for consistency with startup behavior
                let reconnecting_client =
                    connect_relay_chain_with_progress_logging(&relay_url, &self.config)
                        .await
                        .map_err(|e| RelayChainError::ConnectionFailed(e.to_string()))?;

                Ok(Arc::new(RpcClient::new(reconnecting_client)))
            })
            .await
            .cloned()
    }

    /// Connect to a relay chain with reconnection support and progress logging
    async fn connect_relay_chain(
        relay_url: &str,
        _relay_chain_name: &str,
        chain_configs: &polkadot_rest_api_config::ChainConfigs,
        config: &SidecarConfig,
    ) -> Result<
        (
            Arc<OnlineClient<SubstrateConfig>>,
            Arc<RpcClient>,
            ChainInfo,
            polkadot_rest_api_config::ChainConfig,
        ),
        StateError,
    > {
        // Create relay chain RPC client with reconnection support
        let reconnecting_client =
            connect_relay_chain_with_progress_logging(relay_url, config).await?;

        let relay_rpc_client = RpcClient::new(reconnecting_client);
        let relay_legacy_rpc: SubstrateLegacyRpc = LegacyRpcMethods::new(relay_rpc_client.clone());

        // Get relay chain info
        let relay_chain_info = get_chain_info(&relay_legacy_rpc).await?;

        // Load relay chain configuration
        let relay_chain_config = chain_configs
            .get(&relay_chain_info.spec_name)
            .cloned()
            .unwrap_or_else(|| {
                tracing::warn!(
                    "No configuration found for relay chain '{}', using defaults",
                    relay_chain_info.spec_name
                );
                polkadot_rest_api_config::ChainConfig::default()
            });

        // Configure SubstrateConfig with appropriate legacy types
        let relay_subxt_config = match relay_chain_config.legacy_types.as_str() {
            "polkadot" => SubstrateConfig::builder()
                .set_legacy_types(frame_decode::legacy_types::polkadot::relay_chain())
                .build(),
            "kusama-relay" => SubstrateConfig::builder()
                .set_legacy_types(frame_decode::legacy_types::kusama::relay_chain())
                .build(),
            "kusama-asset-hub" => SubstrateConfig::builder()
                .set_legacy_types(frame_decode::legacy_types::kusama::asset_hub())
                .build(),
            _ => SubstrateConfig::new(),
        };

        let relay_client =
            OnlineClient::from_rpc_client_with_config(relay_subxt_config, relay_rpc_client.clone())
                .await
                .map_err(|e| StateError::ConnectionFailed {
                    url: relay_url.to_string(),
                    source: subxt_rpcs::Error::Client(Box::new(std::io::Error::other(
                        e.to_string(),
                    ))),
                })?;

        tracing::info!(
            "Connected to relay chain '{}' at {}",
            relay_chain_info.spec_name,
            relay_url
        );

        Ok((
            Arc::new(relay_client),
            Arc::new(relay_rpc_client),
            relay_chain_info,
            relay_chain_config,
        ))
    }

    /// Make a raw JSON-RPC call to get a header and return the result as a Value
    /// This is needed because subxt-historic's RpcConfig has Header = ()
    pub async fn get_header_json(&self, hash: &str) -> Result<Value, subxt_rpcs::Error> {
        self.rpc_client
            .request("chain_getHeader", rpc_params![hash])
            .await
    }

    /// Make a raw JSON-RPC call to get a full block (header + extrinsics) and return the result as a Value
    /// This is used by the /blocks/{blockId}/extrinsics-raw endpoint to get raw extrinsic data
    pub async fn get_block_json(&self, hash: &str) -> Result<Value, subxt_rpcs::Error> {
        self.rpc_client
            .request("chain_getBlock", rpc_params![hash])
            .await
    }

    /// Make a raw JSON-RPC call to get a block hash at a specific block number
    pub async fn get_block_hash_at_number(
        &self,
        number: u64,
    ) -> Result<Option<String>, subxt_rpcs::Error> {
        let result: Option<String> = self
            .rpc_client
            .request("chain_getBlockHash", rpc_params![number])
            .await?;
        Ok(result)
    }

    /// Make a raw JSON-RPC call to get runtime version at a specific block hash
    pub async fn get_runtime_version_at_hash(
        &self,
        hash: &str,
    ) -> Result<Value, subxt_rpcs::Error> {
        self.rpc_client
            .request("state_getRuntimeVersion", rpc_params![hash])
            .await
    }

    /// Query fee information for an extrinsic at a specific block hash
    ///
    /// Uses the `payment_queryInfo` RPC method to get weight, class, and partial fee
    /// for a given extrinsic. The block hash should typically be the parent block
    /// of the block containing the extrinsic (pre-dispatch state).
    ///
    /// Returns the RuntimeDispatchInfo as JSON with fields:
    /// - weight: { refTime, proofSize } or just a number for older runtimes
    /// - class: "Normal", "Operational", or "Mandatory"
    /// - partialFee: fee amount as string
    pub async fn query_fee_info(
        &self,
        extrinsic_hex: &str,
        block_hash: &str,
    ) -> Result<Value, subxt_rpcs::Error> {
        self.rpc_client
            .request("payment_queryInfo", rpc_params![extrinsic_hex, block_hash])
            .await
    }

    /// Query detailed fee breakdown for an extrinsic at a specific block hash
    ///
    /// Uses the `payment_queryFeeDetails` RPC method to get the fee breakdown needed
    /// for accurate fee calculation. The block hash should typically be the parent block
    /// of the block containing the extrinsic (pre-dispatch state).
    ///
    /// Returns FeeDetails as JSON with fields:
    /// - inclusionFee: { baseFee, lenFee, adjustedWeightFee } or null
    ///
    /// Note: This RPC method is not available on all runtimes. Check the chain's
    /// fee configuration to determine availability based on spec_version.
    pub async fn query_fee_details(
        &self,
        extrinsic_hex: &str,
        block_hash: &str,
    ) -> Result<Value, subxt_rpcs::Error> {
        self.rpc_client
            .request(
                "payment_queryFeeDetails",
                rpc_params![extrinsic_hex, block_hash],
            )
            .await
    }

    /// Execute a runtime API call via `state_call` RPC method.
    ///
    /// This allows calling runtime APIs directly against the historic state at a
    /// specific block hash. The parameters should be SCALE-encoded bytes.
    ///
    /// # Arguments
    /// * `method` - The runtime API method name (e.g., "TransactionPaymentApi_query_info")
    /// * `call_parameters` - SCALE-encoded parameters as hex string (with 0x prefix)
    /// * `block_hash` - The block hash to execute against
    ///
    /// Returns the raw response bytes as a hex string.
    pub async fn state_call(
        &self,
        method: &str,
        call_parameters: &str,
        block_hash: &str,
    ) -> Result<String, subxt_rpcs::Error> {
        self.rpc_client
            .request(
                "state_call",
                rpc_params![method, call_parameters, block_hash],
            )
            .await
    }
}

/// Determine SS58 address format prefix based on chain type and spec name
fn get_ss58_prefix(chain_type: &ChainType, spec_name: &str) -> u16 {
    use polkadot_rest_api_config::{KnownAssetHub, KnownRelayChain};

    match chain_type {
        ChainType::Relay => {
            match KnownRelayChain::from_spec_name(spec_name) {
                Some(KnownRelayChain::Polkadot) => 0,
                Some(KnownRelayChain::Kusama) => 2,
                Some(KnownRelayChain::Westend) => 42,
                Some(KnownRelayChain::Rococo) => 42,
                Some(KnownRelayChain::Paseo) => 42,
                None => 42, // Default to generic substrate
            }
        }
        ChainType::AssetHub => {
            match KnownAssetHub::from_spec_name(spec_name) {
                Some(KnownAssetHub::Polkadot) => 0, // Uses Polkadot's prefix
                Some(KnownAssetHub::Kusama) => 2,   // Uses Kusama's prefix
                Some(KnownAssetHub::Westend) => 42, // Uses Westend's prefix
                Some(KnownAssetHub::Paseo) => 42,   // Uses Paseo's prefix
                None => 42,
            }
        }
        ChainType::Coretime => {
            // Coretime chains inherit SS58 prefix from their parent relay chain
            let name_lower = spec_name.to_lowercase();
            if name_lower.contains("polkadot") {
                0 // Polkadot prefix
            } else if name_lower.contains("kusama") {
                2 // Kusama prefix
            } else {
                42 // Default to generic substrate
            }
        }
        ChainType::Parachain => 42, // Generic substrate for unknown parachains
    }
}

/// Query the chain to get runtime information via RPC
async fn get_chain_info(legacy_rpc: &SubstrateLegacyRpc) -> Result<ChainInfo, StateError> {
    let runtime_version = legacy_rpc
        .state_get_runtime_version(None)
        .await
        .map_err(StateError::RuntimeVersionFailed)?;

    // Extract spec_name from the "other" HashMap
    let spec_name = runtime_version
        .other
        .get("specName")
        .and_then(|v| v.as_str())
        .ok_or(StateError::SpecNameNotFound)?
        .to_string();

    // Determine chain type from spec_name
    let chain_type = ChainType::from_spec_name(&spec_name);

    // Try to get SS58 prefix from system properties first, fall back to hardcoded values
    let ss58_prefix = if let Ok(props) = legacy_rpc.system_properties().await {
        // Try to extract ss58Format from the properties map
        props
            .get("ss58Format")
            .and_then(|v| v.as_u64())
            .map(|v| v as u16)
            .unwrap_or_else(|| get_ss58_prefix(&chain_type, &spec_name))
    } else {
        // If system_properties call fails, use hardcoded mappings
        get_ss58_prefix(&chain_type, &spec_name)
    };

    Ok(ChainInfo {
        chain_type,
        spec_name,
        spec_version: runtime_version.spec_version,
        ss58_prefix,
    })
}

/// Connect to the substrate node with CLI progress indicator.
/// Shows a live progress line that updates every second, independent of log levels.
/// Terminates after 60 seconds with a clear error message.
async fn connect_with_progress_logging(
    url: &str,
    config: &SidecarConfig,
) -> Result<ReconnectingRpcClient, StateError> {
    connect_with_progress_logging_impl(url, config, "Connecting to").await
}

/// Connect to a relay chain with CLI progress indicator.
/// Shows a live progress line that updates every second, independent of log levels.
/// Terminates after 60 seconds with a clear error message.
async fn connect_relay_chain_with_progress_logging(
    url: &str,
    config: &SidecarConfig,
) -> Result<ReconnectingRpcClient, StateError> {
    connect_with_progress_logging_impl(url, config, "Connecting to relay chain at").await
}

/// Internal implementation for connection with progress logging.
/// The `prefix` parameter customizes the progress message (e.g., "Connecting to" vs "Connecting to relay chain at").
async fn connect_with_progress_logging_impl(
    url: &str,
    config: &SidecarConfig,
    prefix: &str,
) -> Result<ReconnectingRpcClient, StateError> {
    use std::io::Write;
    use subxt_rpcs::client::reconnecting_rpc_client::RpcClient as ReconnectingClient;

    let connect_future = ReconnectingClient::builder()
        .retry_policy(
            ExponentialBackoff::from_millis(config.substrate.reconnect_initial_delay_ms).max_delay(
                Duration::from_millis(config.substrate.reconnect_max_delay_ms),
            ),
        )
        .request_timeout(Duration::from_millis(
            config.substrate.reconnect_request_timeout_ms,
        ))
        .build(url);

    tokio::pin!(connect_future);

    let mut interval = tokio::time::interval(Duration::from_secs(1));
    interval.tick().await; // First tick is immediate, skip it

    let mut elapsed_secs = 0u64;
    const TIMEOUT_SECS: u64 = 60;

    // Show initial connection message
    eprint!("\r{} {}...", prefix, url);
    let _ = std::io::stderr().flush();

    loop {
        tokio::select! {
            result = &mut connect_future => {
                // Clear the progress line and show success
                eprint!("\r\x1b[K"); // Clear line
                let _ = std::io::stderr().flush();

                return result.map_err(|source| StateError::ConnectionFailed {
                    url: url.to_string(),
                    source: subxt_rpcs::Error::Client(Box::new(source)),
                });
            }
            _ = interval.tick() => {
                elapsed_secs += 1;

                if elapsed_secs >= TIMEOUT_SECS {
                    // Clear line and print final error
                    eprintln!("\r\x1b[K");
                    eprintln!("Failed to connect to {} after {} seconds.", url, TIMEOUT_SECS);
                    eprintln!("Terminating: no active connection with the RPC node.");

                    return Err(StateError::ConnectionTimeout {
                        url: url.to_string(),
                        timeout_secs: TIMEOUT_SECS,
                    });
                }

                // Update progress line with elapsed time and status message
                let status = match elapsed_secs {
                    0..=9 => "",
                    10..=19 => " (taking longer than usual)",
                    20..=29 => " (taking significantly longer than expected)",
                    30..=39 => " (check if RPC node is running)",
                    _ => " (timing out soon)",
                };

                eprint!(
                    "\r{} {}... {}s{}",
                    prefix, url, elapsed_secs, status
                );
                let _ = std::io::stderr().flush();
            }
        }
    }
}
