mod error;
mod express;
mod log;

pub use error::ConfigError;
pub use express::ExpressConfig;
pub use log::LogConfig;

use serde::Deserialize;

/// Flat structure for loading from environment variables
/// This works better with envy than nested structs
#[derive(Debug, Deserialize)]
struct EnvConfig {
    #[serde(default = "default_express_port")]
    express_port: u16,

    #[serde(default = "default_log_level")]
    log_level: String,
}

fn default_express_port() -> u16 {
    8080
}

fn default_log_level() -> String {
    "info".to_string()
}

/// Main configuration struct
#[derive(Debug, Clone)]
pub struct SidecarConfig {
    pub express: ExpressConfig,
    pub log: LogConfig,
}

impl SidecarConfig {
    /// Load configuration from environment variables
    ///
    /// Looks for variables with `SAS_` prefix:
    /// - SAS_EXPRESS_PORT
    /// - SAS_LOG_LEVEL
    pub fn from_env() -> Result<Self, ConfigError> {
        // Load flat env config
        let env_config = envy::prefixed("SAS_").from_env::<EnvConfig>()?;

        // Map to nested structure
        let config = Self {
            express: ExpressConfig {
                port: env_config.express_port,
            },
            log: LogConfig {
                level: env_config.log_level,
            },
        };

        // Validate
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        self.express.validate()?;
        self.log.validate()?;
        Ok(())
    }
}

impl Default for SidecarConfig {
    fn default() -> Self {
        Self {
            express: ExpressConfig::default(),
            log: LogConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SidecarConfig::default();
        assert_eq!(config.express.port, 8080);
        assert_eq!(config.log.level, "info");
    }
}
