use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to load configuration from enviornment: {0}")]
    EnvError(#[from] envy::Error),

    #[error("Invalid configuration value: {0}")]
    ValidateError(String),
}
