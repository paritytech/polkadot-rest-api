mod error;
mod express;
mod log;
mod substrate;

pub use error::ConfigError;
pub use express::ExpressConfig;
pub use log::LogConfig;
pub use substrate::{ChainType, ChainUrl, SubstrateConfig};

use serde::Deserialize;

/// Flat structure for loading from environment variables
/// This works better with envy than nested structs
#[derive(Debug, Deserialize)]
struct EnvConfig {
    #[serde(default = "default_express_bind_host")]
    express_bind_host: String,

    #[serde(default = "default_express_port")]
    express_port: u16,

    #[serde(default = "default_express_request_limit")]
    express_request_limit: usize,

    #[serde(default = "default_express_keep_alive_timeout")]
    express_keep_alive_timeout: u64,

    #[serde(default = "default_log_level")]
    log_level: String,

    #[serde(default = "default_log_json")]
    log_json: bool,

    #[serde(default = "default_log_strip_ansi")]
    log_strip_ansi: bool,

    #[serde(default = "default_log_write")]
    log_write: bool,

    #[serde(default = "default_log_write_path")]
    log_write_path: String,

    #[serde(default = "default_log_write_max_file_size")]
    log_write_max_file_size: u64,

    #[serde(default = "default_substrate_url")]
    substrate_url: String,

    #[serde(default = "default_substrate_multi_chain_url")]
    substrate_multi_chain_url: String,
}

fn default_express_bind_host() -> String {
    "127.0.0.1".to_string()
}

fn default_express_port() -> u16 {
    8080
}

fn default_express_request_limit() -> usize {
    512_000 // 500kb
}

fn default_express_keep_alive_timeout() -> u64 {
    5000 // 5 seconds in milliseconds
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_json() -> bool {
    false
}

fn default_log_strip_ansi() -> bool {
    false
}

fn default_log_write() -> bool {
    false
}

fn default_log_write_path() -> String {
    "./logs".to_string()
}

fn default_log_write_max_file_size() -> u64 {
    5_242_880 // 5MB
}

fn default_substrate_url() -> String {
    "ws://127.0.0.1:9944".to_string()
}

fn default_substrate_multi_chain_url() -> String {
    String::new()
}

/// Main configuration struct
#[derive(Debug, Clone, Default)]
pub struct SidecarConfig {
    pub express: ExpressConfig,
    pub log: LogConfig,
    pub substrate: SubstrateConfig,
}

impl SidecarConfig {
    /// Load configuration from environment variables
    ///
    /// Looks for variables with `SAS_` prefix:
    /// - SAS_EXPRESS_BIND_HOST
    /// - SAS_EXPRESS_PORT
    /// - SAS_EXPRESS_REQUEST_LIMIT
    /// - SAS_EXPRESS_KEEP_ALIVE_TIMEOUT
    /// - SAS_LOG_LEVEL
    /// - SAS_LOG_JSON
    /// - SAS_LOG_STRIP_ANSI
    /// - SAS_LOG_WRITE
    /// - SAS_LOG_WRITE_PATH
    /// - SAS_LOG_WRITE_MAX_FILE_SIZE
    /// - SAS_SUBSTRATE_URL
    /// - SAS_SUBSTRATE_MULTI_CHAIN_URL
    pub fn from_env() -> Result<Self, ConfigError> {
        // Load flat env config
        let env_config = envy::prefixed("SAS_").from_env::<EnvConfig>()?;

        // Parse multi-chain URLs from JSON
        let multi_chain_urls = if env_config.substrate_multi_chain_url.is_empty() {
            vec![]
        } else {
            serde_json::from_str(&env_config.substrate_multi_chain_url).map_err(|e| {
                ConfigError::ValidateError(format!(
                    "Invalid JSON format for SAS_SUBSTRATE_MULTI_CHAIN_URL: {}",
                    e
                ))
            })?
        };

        // Map to nested structure
        let config = Self {
            express: ExpressConfig {
                bind_host: env_config.express_bind_host,
                port: env_config.express_port,
                request_limit: env_config.express_request_limit,
                keep_alive_timeout: env_config.express_keep_alive_timeout,
            },
            log: LogConfig {
                level: env_config.log_level,
                json: env_config.log_json,
                strip_ansi: env_config.log_strip_ansi,
                write: env_config.log_write,
                write_path: env_config.log_write_path,
                write_max_file_size: env_config.log_write_max_file_size,
            },
            substrate: SubstrateConfig {
                url: env_config.substrate_url,
                multi_chain_urls,
            },
        };

        // Validate
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        self.express.validate()?;
        self.log.validate()?;
        self.substrate.validate()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SidecarConfig::default();
        assert_eq!(config.express.bind_host, "127.0.0.1");
        assert_eq!(config.express.port, 8080);
        assert_eq!(config.log.level, "info");
        assert_eq!(config.log.json, false);
        assert_eq!(config.substrate.url, "ws://127.0.0.1:9944");
        assert_eq!(config.substrate.multi_chain_urls.len(), 0);
    }

    #[test]
    fn test_from_env_with_multi_chain() {
        unsafe {
            std::env::set_var("SAS_EXPRESS_PORT", "8080");
            std::env::set_var("SAS_LOG_LEVEL", "info");
            std::env::set_var("SAS_SUBSTRATE_URL", "ws://localhost:9944");
            std::env::set_var(
                "SAS_SUBSTRATE_MULTI_CHAIN_URL",
                r#"[{"url":"ws://polkadot:9944","type":"relay"},{"url":"ws://asset-hub:9944","type":"assethub"}]"#,
            );
        }

        let config = SidecarConfig::from_env().unwrap();
        assert_eq!(config.substrate.multi_chain_urls.len(), 2);
        assert_eq!(
            config.substrate.multi_chain_urls[0].url,
            "ws://polkadot:9944"
        );
        assert_eq!(
            config.substrate.multi_chain_urls[0].chain_type,
            ChainType::Relay
        );
        assert_eq!(
            config.substrate.multi_chain_urls[1].url,
            "ws://asset-hub:9944"
        );
        assert_eq!(
            config.substrate.multi_chain_urls[1].chain_type,
            ChainType::AssetHub
        );

        // Clean up
        unsafe {
            std::env::remove_var("SAS_EXPRESS_PORT");
            std::env::remove_var("SAS_LOG_LEVEL");
            std::env::remove_var("SAS_SUBSTRATE_URL");
            std::env::remove_var("SAS_SUBSTRATE_MULTI_CHAIN_URL");
        }
    }

    #[test]
    fn test_from_env_invalid_multi_chain_json() {
        unsafe {
            std::env::set_var("SAS_SUBSTRATE_URL", "ws://localhost:9944");
            std::env::set_var("SAS_SUBSTRATE_MULTI_CHAIN_URL", "not-valid-json");
        }

        let result = SidecarConfig::from_env();
        assert!(result.is_err());

        // Clean up
        unsafe {
            std::env::remove_var("SAS_SUBSTRATE_URL");
            std::env::remove_var("SAS_SUBSTRATE_MULTI_CHAIN_URL");
        }
    }
}
