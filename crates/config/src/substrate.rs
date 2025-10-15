use crate::ConfigError;

#[derive(Debug, Clone)]
pub struct SubstrateConfig {
    /// Substrate node WebSocket or HTTP URL
    ///
    /// Env: SAS_SUBSTRATE_URL
    /// Valid schemes: ws://, wss://, http://, https://
    /// Default: ws://127.0.0.1:9944
    pub url: String,
}

impl SubstrateConfig {
    pub(crate) fn validate(&self) -> Result<(), ConfigError> {
        if self.url.is_empty() {
            return Err(ConfigError::ValidateError(
                "Substrate URL cannot be empty".to_string(),
            ));
        }

        // Parse URL to check format
        let parsed = url::Url::parse(&self.url).map_err(|e| {
            ConfigError::ValidateError(format!("Invalid substrate URL '{}': {}", self.url, e))
        })?;

        // Check scheme
        match parsed.scheme() {
            "ws" | "wss" | "http" | "https" => Ok(()),
            scheme => Err(ConfigError::ValidateError(format!(
                "Invalid URL scheme '{}'. Must be ws://, wss://, http://, or https://",
                scheme
            ))),
        }
    }
}

impl Default for SubstrateConfig {
    fn default() -> Self {
        Self {
            url: "ws://127.0.0.1:9944".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_substrate_config() {
        let config = SubstrateConfig::default();
        assert_eq!(config.url, "ws://127.0.0.1:9944");
    }

    #[test]
    fn test_validate_empty_url() {
        let config = SubstrateConfig {
            url: "".to_string(),
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_invalid_url_format() {
        let config = SubstrateConfig {
            url: "not-a-valid-url".to_string(),
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_invalid_scheme() {
        let config = SubstrateConfig {
            url: "ftp://localhost:9944".to_string(),
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_valid_ws_url() {
        let config = SubstrateConfig {
            url: "ws://localhost:9944".to_string(),
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_valid_wss_url() {
        let config = SubstrateConfig {
            url: "wss://polkadot.api.io".to_string(),
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_valid_http_url() {
        let config = SubstrateConfig {
            url: "http://localhost:9933".to_string(),
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_valid_https_url() {
        let config = SubstrateConfig {
            url: "https://rpc.polkadot.io".to_string(),
        };
        assert!(config.validate().is_ok());
    }
}
