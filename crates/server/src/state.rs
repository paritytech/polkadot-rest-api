use config::SidecarConfig;

#[derive(Clone)]
pub struct AppState {
    pub config: SidecarConfig,
}

impl AppState {
    pub async fn new() -> anyhow::Result<Self> {
        let config = SidecarConfig::from_env()?;
        Ok(Self { config })
    }
}
