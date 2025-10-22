use config::SidecarConfig;
use std::sync::Arc;
use subxt_historic::{OnlineClient, SubstrateConfig};

#[derive(Clone)]
pub struct AppState {
    pub config: SidecarConfig,
    #[allow(dead_code)] // Will be used when implementing endpoints
    pub client: Arc<OnlineClient<SubstrateConfig>>,
}

impl AppState {
    pub async fn new() -> anyhow::Result<Self> {
        let config = SidecarConfig::from_env()?;
        Self::new_with_config(config).await
    }

    pub async fn new_with_config(config: SidecarConfig) -> anyhow::Result<Self> {
        // Create subxt-historic config
        let subxt_config = SubstrateConfig::new();

        // Connect to archive node using the primary substrate URL
        let client = OnlineClient::from_insecure_url(subxt_config, &config.substrate.url)
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to connect to substrate node at {}: {}",
                    config.substrate.url,
                    e
                )
            })?;

        Ok(Self {
            config,
            client: Arc::new(client),
        })
    }
}
