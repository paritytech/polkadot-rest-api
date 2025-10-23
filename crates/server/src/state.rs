use config::{ChainType, SidecarConfig};
use std::sync::Arc;
use subxt_historic::{OnlineClient, SubstrateConfig};
use subxt_rpcs::{LegacyRpcMethods, RpcClient};

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
    pub chain_info: ChainInfo,
}

impl AppState {
    pub async fn new() -> anyhow::Result<Self> {
        let config = SidecarConfig::from_env()?;
        Self::new_with_config(config).await
    }

    pub async fn new_with_config(config: SidecarConfig) -> anyhow::Result<Self> {
        // Create RPC client first - we'll use it for both historic client and legacy RPC
        let rpc_client = RpcClient::from_insecure_url(&config.substrate.url)
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to connect to substrate node at {}: {}",
                    config.substrate.url,
                    e
                )
            })?;

        let legacy_rpc = LegacyRpcMethods::new(rpc_client.clone());
        let subxt_config = SubstrateConfig::new();
        let client = OnlineClient::from_rpc_client(subxt_config, rpc_client);
        let chain_info = get_chain_info(&legacy_rpc).await?;

        Ok(Self {
            config,
            client: Arc::new(client),
            legacy_rpc: Arc::new(legacy_rpc),
            chain_info,
        })
    }
}

/// Query the chain to get runtime information via RPC
async fn get_chain_info(
    legacy_rpc: &LegacyRpcMethods<SubstrateConfig>,
) -> anyhow::Result<ChainInfo> {
    let runtime_version = legacy_rpc
        .state_get_runtime_version(None)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get runtime version: {}", e))?;

    // Extract spec_name from the "other" HashMap
    let spec_name = runtime_version
        .other
        .get("specName")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("spec_name not found in runtime version"))?
        .to_string();

    // Determine chain type based on spec_name
    let chain_type = determine_chain_type(&spec_name);

    Ok(ChainInfo {
        chain_type,
        spec_name,
        spec_version: runtime_version.spec_version,
    })
}

/// Determine the chain type based on the spec_name
fn determine_chain_type(spec_name: &str) -> ChainType {
    let name_lower = spec_name.to_lowercase();

    // Check for Asset Hub first (most specific)
    // Legacy names: statemint (Polkadot), statemine (Kusama), westmint (Westend)
    // New names: asset-hub-polkadot, asset-hub-kusama, asset-hub-westend
    if name_lower == "statemint"
        || name_lower == "statemine"
        || name_lower == "westmint"
        || name_lower.contains("asset-hub")
        || name_lower.contains("assethub")
    {
        return ChainType::AssetHub;
    }

    // Check for relay chains (exact matches)
    if name_lower == "polkadot"
        || name_lower == "kusama"
        || name_lower == "westend"
        || name_lower == "paseo"
    {
        return ChainType::Relay;
    }

    // Default to Parachain for everything else
    ChainType::Parachain
}
