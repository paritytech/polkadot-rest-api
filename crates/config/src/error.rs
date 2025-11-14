use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to load configuration from environment")]
    EnvError(#[from] envy::Error),

    #[error("Express configuration error")]
    ExpressError(#[from] crate::express::ExpressError),

    #[error("Log configuration error")]
    LogError(#[from] crate::log::LogError),

    #[error("Substrate configuration error")]
    SubstrateError(#[from] crate::substrate::SubstrateError),

    #[error("Metrics configuration error")]
    MetricsError(#[from] crate::metrics::MetricsError),

    #[error("Invalid multi-chain URL JSON")]
    InvalidMultiChainJson(#[from] serde_json::Error),
}
