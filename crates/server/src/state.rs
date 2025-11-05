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
