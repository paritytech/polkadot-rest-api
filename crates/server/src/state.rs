use crate::handlers::blocks::types::GetBlockError;
use crate::routes::RouteRegistry;
use crate::utils::{
    QueryFeeDetailsCache, RuntimeDispatchInfoRaw, WeightRaw, dispatch_class_from_u8,
};
use config::{ChainType, SidecarConfig};
use parity_scale_codec::{Compact, Decode, Encode};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use subxt::config::RpcConfigFor;
use subxt::{OnlineClient, SubstrateConfig};
use subxt_rpcs::client::reconnecting_rpc_client::{
    ExponentialBackoff, RpcClient as ReconnectingRpcClient,
};
use subxt_rpcs::{LegacyRpcMethods, RpcClient, rpc_params};

/// Type alias for LegacyRpcMethods with correct RpcConfig wrapper
pub type SubstrateLegacyRpc = LegacyRpcMethods<RpcConfigFor<SubstrateConfig>>;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StateError {
    #[error("Failed to load configuration")]
    ConfigLoadFailed(#[from] config::ConfigError),

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
    pub chain_configs: Arc<config::ChainConfigs>,
    /// Complete configuration with optional relay chain
    pub chain_config: Arc<config::Config>,
    /// Registry of all available routes for introspection
    pub route_registry: RouteRegistry,
    /// Relay Chain RPC client (only present when multi-chain is configured with a relay chain)
    pub relay_chain_rpc: Option<Arc<SubstrateLegacyRpc>>,
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
        let chain_configs = Arc::new(config::ChainConfigs::default());

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
                match Self::connect_relay_chain(relay_url, relay_chain_name, &chain_configs).await {
                    Ok((client, rpc, info, config)) => {
                        (Some(client), Some(rpc), Some(info), Some(config))
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to connect to relay chain at {}: {}. Continuing without relay chain support.",
                            relay_url,
                            e
                        );
                        (None, None, None, None)
                    }
                }
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
            Arc::new(config::Config::with_relay_chain(
                chain_chain_config,
                rc_config,
            ))
        } else {
            Arc::new(config::Config::single_chain(chain_chain_config))
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

    /// Connect to a relay chain
    async fn connect_relay_chain(
        relay_url: &str,
        _relay_chain_name: &str,
        chain_configs: &config::ChainConfigs,
    ) -> Result<
        (
            Arc<OnlineClient<SubstrateConfig>>,
            Arc<RpcClient>,
            ChainInfo,
            config::ChainConfig,
        ),
        StateError,
    > {
        // Create relay chain RPC client
        let relay_rpc_client = RpcClient::from_insecure_url(relay_url)
            .await
            .map_err(|source| StateError::ConnectionFailed {
                url: relay_url.to_string(),
                source,
            })?;

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
                config::ChainConfig::default()
            });

        // Configure SubstrateConfig with appropriate legacy types
        let relay_subxt_config = match relay_chain_config.legacy_types.as_str() {
            "polkadot" => SubstrateConfig::builder()
                .set_legacy_types(frame_decode::legacy_types::polkadot::relay_chain())
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

    /// Make a raw JSON-RPC call to get a full block from the relay chain
    pub async fn get_relay_block_json(&self, hash: &str) -> Result<Value, GetBlockError> {
        let rpc_client = self
            .relay_rpc_client
            .as_ref()
            .ok_or(GetBlockError::RelayChainNotConfigured)?;
        rpc_client
            .request("chain_getBlock", rpc_params![hash])
            .await
            .map_err(GetBlockError::BlockFetchFailed)
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

    /// Query fee information for an extrinsic using the TransactionPaymentApi runtime API.
    ///
    /// This is an alternative to `query_fee_info` that uses `state_call` to call the
    /// runtime API directly. This method works for historic blocks where the
    /// `payment_queryInfo` RPC method might not be available.
    ///
    /// # Arguments
    /// * `extrinsic_bytes` - The raw extrinsic bytes (not hex encoded)
    /// * `block_hash` - The block hash to execute against (parent block for pre-dispatch state)
    ///
    /// Returns RuntimeDispatchInfo containing weight, class, and partialFee.
    pub async fn query_fee_info_via_runtime_api(
        &self,
        extrinsic_bytes: &[u8],
        block_hash: &str,
    ) -> Result<RuntimeDispatchInfoRaw, subxt_rpcs::Error> {
        let mut params = extrinsic_bytes.to_vec();
        let len = extrinsic_bytes.len() as u32;
        len.encode_to(&mut params);

        let params_hex = format!("0x{}", hex::encode(&params));

        let result_hex: String = self
            .state_call("TransactionPaymentApi_query_info", &params_hex, block_hash)
            .await?;

        let result_bytes = hex::decode(result_hex.trim_start_matches("0x")).map_err(|e| {
            subxt_rpcs::Error::Client(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to decode hex: {}", e),
            )))
        })?;

        // Try to decode as legacy RuntimeDispatchInfo (with V1 weight) FIRST
        // Format: { weight: u64, class: u8, partial_fee: u128 } = exactly 25 bytes
        // V1 is tried first because it has a fixed size and validates cleanly.
        // The V2 Compact decoder is too permissive and will "succeed" with garbage on V1 data.
        if result_bytes.len() == 25
            && let Ok((weight, class, partial_fee)) =
                <(u64, u8, u128)>::decode(&mut &result_bytes[..])
            && class <= 2
        {
            return Ok(RuntimeDispatchInfoRaw {
                weight: WeightRaw::V1(weight),
                class: dispatch_class_from_u8(class),
                partial_fee,
            });
        }

        // Try to decode as modern RuntimeDispatchInfo (with V2 weight)
        // Format: { weight: { ref_time: Compact<u64>, proof_size: Compact<u64> }, class: u8, partial_fee: u128 }
        if let Ok((ref_time, proof_size, class, partial_fee)) =
            <(Compact<u64>, Compact<u64>, u8, u128)>::decode(&mut &result_bytes[..])
            && class <= 2
        {
            return Ok(RuntimeDispatchInfoRaw {
                weight: WeightRaw::V2 {
                    ref_time: ref_time.0,
                    proof_size: proof_size.0,
                },
                class: dispatch_class_from_u8(class),
                partial_fee,
            });
        }

        // If V2 failed validation, try V1 without length check as fallback
        if let Ok((weight, class, partial_fee)) = <(u64, u8, u128)>::decode(&mut &result_bytes[..])
            && class <= 2
        {
            return Ok(RuntimeDispatchInfoRaw {
                weight: WeightRaw::V1(weight),
                class: dispatch_class_from_u8(class),
                partial_fee,
            });
        }

        Err(subxt_rpcs::Error::Client(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Failed to decode RuntimeDispatchInfo from state_call response",
        ))))
    }
}

/// Determine SS58 address format prefix based on chain type and spec name
fn get_ss58_prefix(chain_type: &ChainType, spec_name: &str) -> u16 {
    use config::{KnownAssetHub, KnownRelayChain};

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
    eprint!("\rConnecting to {}...", url);
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
                    "\rConnecting to {}... {}s{}",
                    url, elapsed_secs, status
                );
                let _ = std::io::stderr().flush();
            }
        }
    }
}
