use crate::utils::RcBlockError;
use config::{ChainType, SidecarConfig};
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
        let subxt_config = SubstrateConfig::new();
        let client = OnlineClient::from_rpc_client(subxt_config, rpc_client.clone());
        let chain_info = get_chain_info(&legacy_rpc).await?;

        Ok(Self {
            config,
            client: Arc::new(client),
            legacy_rpc: Arc::new(legacy_rpc),
            rpc_client: Arc::new(rpc_client),
            chain_info,
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

    /// Check if Asset Hub connection is available in multi-chain config
    pub fn has_asset_hub(&self) -> bool {
        self.config
            .substrate
            .multi_chain_urls
            .iter()
            .any(|chain_url| chain_url.chain_type == ChainType::AssetHub)
            || self.chain_info.chain_type == ChainType::AssetHub
    }

    /// Get Asset Hub RPC client from multi-chain config
    ///
    /// Returns the RPC client for the Asset Hub node if available.
    /// If the primary connection is to Asset Hub, returns that client.
    /// Otherwise, looks for Asset Hub in multi-chain URLs.
    pub async fn get_asset_hub_rpc_client(&self) -> Result<Arc<RpcClient>, RcBlockError> {
        // If we're already connected to Asset Hub, use that client
        if self.chain_info.chain_type == ChainType::AssetHub {
            return Ok(self.rpc_client.clone());
        }

        // Otherwise, look for Asset Hub in multi-chain URLs
        let ah_url = self
            .config
            .substrate
            .multi_chain_urls
            .iter()
            .find(|chain_url| chain_url.chain_type == ChainType::AssetHub)
            .map(|chain_url| &chain_url.url)
            .ok_or(RcBlockError::AssetHubNotAvailable)?;

        // Create RPC client for Asset Hub
        let ah_rpc_client = RpcClient::from_insecure_url(ah_url)
            .await
            .map_err(RcBlockError::AssetHubConnectionFailed)?;

        Ok(Arc::new(ah_rpc_client))
    }

    /// Get Asset Hub legacy RPC methods
    pub async fn get_asset_hub_legacy_rpc(
        &self,
    ) -> Result<Arc<LegacyRpcMethods<SubstrateConfig>>, RcBlockError> {
        let ah_rpc_client = self.get_asset_hub_rpc_client().await?;
        Ok(Arc::new(LegacyRpcMethods::new((*ah_rpc_client).clone())))
    }

    /// Get Asset Hub subxt client
    pub async fn get_asset_hub_subxt_client(
        &self,
    ) -> Result<Arc<OnlineClient<SubstrateConfig>>, RcBlockError> {
        // If we're already connected to Asset Hub, use that client
        if self.chain_info.chain_type == ChainType::AssetHub {
            return Ok(self.client.clone());
        }

        // Otherwise, look for Asset Hub in multi-chain URLs
        let ah_url = self
            .config
            .substrate
            .multi_chain_urls
            .iter()
            .find(|chain_url| chain_url.chain_type == ChainType::AssetHub)
            .map(|chain_url| &chain_url.url)
            .ok_or(RcBlockError::AssetHubNotAvailable)?;

        // Create RPC client for Asset Hub
        let ah_rpc_client = RpcClient::from_insecure_url(ah_url)
            .await
            .map_err(RcBlockError::AssetHubConnectionFailed)?;

        // Create subxt client from RPC client
        let subxt_config = SubstrateConfig::new();
        let ah_client = OnlineClient::from_rpc_client(subxt_config, ah_rpc_client);

        Ok(Arc::new(ah_client))
    }

    /// Get Relay Chain subxt client
    pub async fn get_relay_chain_subxt_client(
        &self,
    ) -> Result<Arc<OnlineClient<SubstrateConfig>>, RcBlockError> {
        // If we're already connected to Relay Chain, use that client
        if self.chain_info.chain_type == ChainType::Relay {
            return Ok(self.client.clone());
        }

        // Otherwise, look for Relay Chain in multi-chain URLs
        let rc_url = self
            .config
            .substrate
            .multi_chain_urls
            .iter()
            .find(|chain_url| chain_url.chain_type == ChainType::Relay)
            .map(|chain_url| &chain_url.url)
            .ok_or(RcBlockError::AssetHubNotAvailable)?;

        // Create RPC client for Relay Chain
        // Handle both secure (wss, https) and insecure (ws, http) URLs
        let rc_rpc_client = if rc_url.starts_with("wss://") || rc_url.starts_with("https://") {
            RpcClient::from_url(rc_url)
                .await
                .map_err(RcBlockError::AssetHubConnectionFailed)?
        } else {
            RpcClient::from_insecure_url(rc_url)
                .await
                .map_err(RcBlockError::AssetHubConnectionFailed)?
        };

        // Create subxt client from RPC client
        let subxt_config = SubstrateConfig::new();
        let rc_client = OnlineClient::from_rpc_client(subxt_config, rc_rpc_client);

        Ok(Arc::new(rc_client))
    }

    /// Get Relay Chain RPC client from multi-chain config
    ///
    /// Returns the RPC client for the Relay Chain node if available.
    /// If the primary connection is to Relay Chain, returns that client.
    /// Otherwise, looks for Relay Chain in multi-chain URLs.
    pub async fn get_relay_chain_rpc_client(&self) -> Result<Arc<RpcClient>, RcBlockError> {
        // If we're already connected to Relay Chain, use that client
        if self.chain_info.chain_type == ChainType::Relay {
            return Ok(self.rpc_client.clone());
        }

        // Otherwise, look for Relay Chain in multi-chain URLs
        let rc_url = self
            .config
            .substrate
            .multi_chain_urls
            .iter()
            .find(|chain_url| chain_url.chain_type == ChainType::Relay)
            .map(|chain_url| &chain_url.url)
            .ok_or(RcBlockError::AssetHubNotAvailable)?;

        // Create RPC client for Relay Chain
        // Handle both secure (wss, https) and insecure (ws, http) URLs
        let rc_rpc_client = if rc_url.starts_with("wss://") || rc_url.starts_with("https://") {
            RpcClient::from_url(rc_url)
                .await
                .map_err(RcBlockError::AssetHubConnectionFailed)?
        } else {
            RpcClient::from_insecure_url(rc_url)
                .await
                .map_err(RcBlockError::AssetHubConnectionFailed)?
        };

        Ok(Arc::new(rc_rpc_client))
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

    Ok(ChainInfo {
        chain_type,
        spec_name,
        spec_version: runtime_version.spec_version,
    })
}
