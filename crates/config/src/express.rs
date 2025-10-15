use crate::ConfigError;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ExpressConfig {
    /// Port to bind the HTTP server to
    ///
    /// Env: SAS_EXPRESS_PORT
    /// Default: 8080
    pub port: u16,
}

fn default_port() -> u16 {
    8080
}

impl ExpressConfig {
    pub(crate) fn validate(&self) -> Result<(), ConfigError> {
        if self.port == 0 {
            return Err(ConfigError::ValidateError(
                "Express port cannot be 0".to_string(),
            ));
        }

        Ok(())
    }
}

impl Default for ExpressConfig {
    fn default() -> Self {
        Self {
            port: default_port(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_express_config() {
        let config = ExpressConfig::default();
        assert_eq!(config.port, 8080);
    }

    #[test]
    fn test_validate_port_zero() {
        let config = ExpressConfig { port: 0 };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_port_valid() {
        let config = ExpressConfig { port: 3000 };
        assert!(config.validate().is_ok())
    }
}
