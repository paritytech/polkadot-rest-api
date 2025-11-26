use crate::utils::QueryFeeDetailsCache;
use config::{ChainType, KnownRelayChain, SidecarConfig};
use serde_json::Value;
use std::sync::Arc;
use subxt_historic::{OnlineClient, SubstrateConfig};
use subxt_rpcs::{LegacyRpcMethods, RpcClient, rpc_params};
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
    pub legacy_rpc: Arc<LegacyRpcMethods<SubstrateConfig>>,
    pub rpc_client: Arc<RpcClient>,
    pub chain_info: ChainInfo,
    /// Cache for tracking queryFeeDetails availability per spec version
    pub fee_details_cache: Arc<QueryFeeDetailsCache>,
}

impl AppState {
    pub async fn new() -> Result<Self, StateError> {
        let config = SidecarConfig::from_env()?;
        Self::new_with_config(config).await
    }

    pub async fn new_with_config(config: SidecarConfig) -> Result<Self, StateError> {
        // Create RPC client first - we'll use it for both historic client and legacy RPC
        let rpc_client = RpcClient::from_insecure_url(&config.substrate.url)
            .await
            .map_err(|source| StateError::ConnectionFailed {
                url: config.substrate.url.clone(),
                source,
            })?;

        let legacy_rpc = LegacyRpcMethods::new(rpc_client.clone());

        // Get chain info first to determine which legacy types to load
        let chain_info = get_chain_info(&legacy_rpc).await?;

        // Configure SubstrateConfig with appropriate legacy types based on chain
        let subxt_config = match chain_info.chain_type.as_relay_chain(&chain_info.spec_name) {
            Some(KnownRelayChain::Polkadot) => {
                // Load Polkadot-specific legacy types for historic block support
                SubstrateConfig::new()
                    .set_legacy_types(subxt_historic::config::polkadot::legacy_types())
            }
            Some(KnownRelayChain::Kusama)
            | Some(KnownRelayChain::Westend)
            | Some(KnownRelayChain::Rococo)
            | Some(KnownRelayChain::Paseo) => {
                // For other known relay chains, use Polkadot types as fallback
                // TODO: Add chain-specific legacy types when available
                SubstrateConfig::new()
                    .set_legacy_types(subxt_historic::config::polkadot::legacy_types())
            }
            None => {
                // For parachains and unknown chains, use empty legacy types
                SubstrateConfig::new()
            }
        };

        let client = OnlineClient::from_rpc_client(subxt_config, rpc_client.clone());

        Ok(Self {
            config,
            client: Arc::new(client),
            legacy_rpc: Arc::new(legacy_rpc),
            rpc_client: Arc::new(rpc_client),
            chain_info,
            fee_details_cache: Arc::new(QueryFeeDetailsCache::new()),
        })
    }

    /// Make a raw JSON-RPC call to get a header and return the result as a Value
    /// This is needed because subxt-historic's RpcConfig has Header = ()
    pub async fn get_header_json(&self, hash: &str) -> Result<Value, subxt_rpcs::Error> {
        self.rpc_client
            .request("chain_getHeader", rpc_params![hash])
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
async fn get_chain_info(
    legacy_rpc: &LegacyRpcMethods<SubstrateConfig>,
) -> Result<ChainInfo, StateError> {
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
