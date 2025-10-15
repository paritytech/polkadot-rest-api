mod express;
mod log;
mod error;

pub use error::ConfigError;
pub use express::ExpressConfig;
pub use log::LogConfig;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct SidecarConfig {
    #[serde(default)]
    pub express: ExpressConfig,

    #[serde(default)]
    pub log: LogConfig,
}

impl SidecarConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        let config = envy::prefixed("SAS_").from_env::<Self>()?;
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
            log: LogConfig::default()
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
