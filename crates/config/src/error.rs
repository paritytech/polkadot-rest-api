use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to load configuration from environment: {0}")]
    EnvError(#[from] envy::Error),

    #[error("Express configuration error: {0}")]
    ExpressError(#[from] crate::express::ExpressError),

    #[error("Log configuration error: {0}")]
    LogError(#[from] crate::log::LogError),

    #[error("Substrate configuration error: {0}")]
    SubstrateError(#[from] crate::substrate::SubstrateError),

    #[error("Invalid multi-chain URL JSON: {0}")]
    InvalidMultiChainJson(String),
}
