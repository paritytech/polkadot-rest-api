mod chain;
mod error;
mod express;
mod log;
mod metrics;
mod spec_versions;
mod substrate;

pub use chain::{
    ChainConfig, ChainConfigError, ChainConfigs, Hasher,
    QueryFeeDetailsStatus as ChainQueryFeeDetailsStatus,
};
pub use error::ConfigError;
pub use express::{ExpressConfig, ExpressError};
pub use log::{LogConfig, LogError};
pub use metrics::{MetricsConfig, MetricsError};
pub use spec_versions::SpecVersionChanges;
pub use substrate::{
    ChainType, ChainUrl, KnownAssetHub, KnownRelayChain, SubstrateConfig, SubstrateError,
};

use serde::Deserialize;

/// Complete configuration structure for chain + optional relay chain
///
/// This struct supports dual-connection mode where a parachain can be connected
/// alongside its relay chain for operations like:
/// - Parachain inclusion tracking
/// - Historic staking queries  
/// - useRcBlock functionality
#[derive(Debug, Clone)]
pub struct Config {
    /// Primary chain configuration
    pub chain: ChainConfig,

    /// Optional relay chain configuration (for parachains)
    pub rc: Option<ChainConfig>,
}

impl Config {
    /// Create a single-chain config (no relay chain)
    pub fn single_chain(chain: ChainConfig) -> Self {
        Self { chain, rc: None }
    }

    /// Create a dual-chain config (parachain + relay chain)
    pub fn with_relay_chain(chain: ChainConfig, relay_chain: ChainConfig) -> Self {
        Self {
            chain,
            rc: Some(relay_chain),
        }
    }

    /// Check if relay chain is configured
    pub fn has_relay_chain(&self) -> bool {
        self.rc.is_some()
    }
}

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

    #[serde(default = "default_express_block_fetch_concurrency")]
    express_block_fetch_concurrency: usize,

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

    #[serde(default = "default_log_write_max_files")]
    log_write_max_files: usize,

    #[serde(default = "default_substrate_url")]
    substrate_url: String,

    #[serde(default)]
    relay_chain_url: Option<String>,

    #[serde(default = "default_substrate_multi_chain_url")]
    substrate_multi_chain_url: String,

    #[serde(default = "default_substrate_reconnect_initial_delay_ms")]
    substrate_reconnect_initial_delay_ms: u64,

    #[serde(default = "default_substrate_reconnect_max_delay_ms")]
    substrate_reconnect_max_delay_ms: u64,

    #[serde(default = "default_substrate_reconnect_request_timeout_ms")]
    substrate_reconnect_request_timeout_ms: u64,

    #[serde(default = "default_metrics_enabled")]
    metrics_enabled: bool,

    #[serde(default = "default_metrics_prom_host")]
    metrics_prom_host: String,

    #[serde(default = "default_metrics_prom_port")]
    metrics_prom_port: u16,

    #[serde(default = "default_metrics_prometheus_prefix")]
    metrics_prometheus_prefix: String,

    #[serde(default = "default_metrics_loki_host")]
    metrics_loki_host: String,

    #[serde(default = "default_metrics_loki_port")]
    metrics_loki_port: u16,

    #[serde(default = "default_metrics_include_queryparams")]
    metrics_include_queryparams: bool,
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

fn default_express_block_fetch_concurrency() -> usize {
    10
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

fn default_log_write_max_files() -> usize {
    5
}

fn default_substrate_url() -> String {
    "ws://127.0.0.1:9944".to_string()
}

fn default_substrate_multi_chain_url() -> String {
    String::new()
}

fn default_substrate_reconnect_initial_delay_ms() -> u64 {
    100
}

fn default_substrate_reconnect_max_delay_ms() -> u64 {
    10000
}

fn default_substrate_reconnect_request_timeout_ms() -> u64 {
    30000
}

fn default_metrics_enabled() -> bool {
    false
}

fn default_metrics_prom_host() -> String {
    "127.0.0.1".to_string()
}

fn default_metrics_prom_port() -> u16 {
    9100
}

fn default_metrics_prometheus_prefix() -> String {
    "polkadot_rest_api".to_string()
}

fn default_metrics_loki_host() -> String {
    "127.0.0.1".to_string()
}

fn default_metrics_loki_port() -> u16 {
    3100
}

fn default_metrics_include_queryparams() -> bool {
    false
}

/// Main configuration struct
#[derive(Debug, Clone, Default)]
pub struct SidecarConfig {
    pub express: ExpressConfig,
    pub log: LogConfig,
    pub substrate: SubstrateConfig,
    pub metrics: MetricsConfig,
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
    /// - SAS_LOG_WRITE_MAX_FILES
    /// - SAS_SUBSTRATE_URL
    /// - SAS_SUBSTRATE_MULTI_CHAIN_URL
    /// - SAS_SUBSTRATE_RECONNECT_INITIAL_DELAY_MS
    /// - SAS_SUBSTRATE_RECONNECT_MAX_DELAY_MS
    /// - SAS_SUBSTRATE_RECONNECT_REQUEST_TIMEOUT_MS
    /// - SAS_METRICS_ENABLED
    /// - SAS_METRICS_PROM_HOST
    /// - SAS_METRICS_PROM_PORT
    /// - SAS_METRICS_PROMETHEUS_PREFIX
    /// - SAS_METRICS_LOKI_HOST
    /// - SAS_METRICS_LOKI_PORT
    /// - SAS_METRICS_INCLUDE_QUERYPARAMS
    pub fn from_env() -> Result<Self, ConfigError> {
        // Load flat env config
        let env_config = envy::prefixed("SAS_").from_env::<EnvConfig>()?;

        // Parse multi-chain URLs from JSON
        let multi_chain_urls = if env_config.substrate_multi_chain_url.is_empty() {
            vec![]
        } else {
            serde_json::from_str(&env_config.substrate_multi_chain_url)?
        };

        // Map to nested structure
        let config = Self {
            express: ExpressConfig {
                bind_host: env_config.express_bind_host,
                port: env_config.express_port,
                request_limit: env_config.express_request_limit,
                keep_alive_timeout: env_config.express_keep_alive_timeout,
                block_fetch_concurrency: env_config.express_block_fetch_concurrency,
            },
            log: LogConfig {
                level: env_config.log_level,
                json: env_config.log_json,
                strip_ansi: env_config.log_strip_ansi,
                write: env_config.log_write,
                write_path: env_config.log_write_path,
                write_max_file_size: env_config.log_write_max_file_size,
                write_max_files: env_config.log_write_max_files,
            },
            substrate: SubstrateConfig {
                url: env_config.substrate_url,
                relay_chain_url: env_config.relay_chain_url,
                multi_chain_urls,
                reconnect_initial_delay_ms: env_config.substrate_reconnect_initial_delay_ms,
                reconnect_max_delay_ms: env_config.substrate_reconnect_max_delay_ms,
                reconnect_request_timeout_ms: env_config.substrate_reconnect_request_timeout_ms,
            },
            metrics: MetricsConfig {
                enabled: env_config.metrics_enabled,
                prom_host: env_config.metrics_prom_host,
                prom_port: env_config.metrics_prom_port,
                prometheus_prefix: env_config.metrics_prometheus_prefix,
                loki_host: env_config.metrics_loki_host,
                loki_port: env_config.metrics_loki_port,
                include_queryparams: env_config.metrics_include_queryparams,
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
        self.metrics.validate()?;
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
        assert!(!config.log.json);
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

    #[test]
    fn test_config_single_chain() {
        let chain_config = ChainConfig::default();
        let config = Config::single_chain(chain_config);
        assert!(!config.has_relay_chain());
        assert!(config.rc.is_none());
    }

    #[test]
    fn test_config_with_relay_chain() {
        let chain_config = ChainConfig::default();
        let relay_config = ChainConfig::default();
        let config = Config::with_relay_chain(chain_config, relay_config);
        assert!(config.has_relay_chain());
        assert!(config.rc.is_some());
    }

    #[test]
    fn test_config_relay_chain_is_optional() {
        let chain_config = ChainConfig::default();
        let config = Config {
            chain: chain_config,
            rc: None,
        };
        assert!(!config.has_relay_chain());
    }
}
