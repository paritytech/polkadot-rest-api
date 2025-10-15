use crate::ConfigError;
use serde::Deserialize;

/// Chain type identifier
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ChainType {
    Relay,
    #[serde(rename = "assethub")]
    AssetHub,
    Parachain,
}

/// Multi-chain URL configuration
#[derive(Debug, Clone, Deserialize)]
pub struct ChainUrl {
    pub url: String,
    #[serde(rename = "type")]
    pub chain_type: ChainType,
}

#[derive(Debug, Clone)]
pub struct SubstrateConfig {
    /// Primary substrate node WebSocket or HTTP URL
    ///
    /// Env: SAS_SUBSTRATE_URL
    /// Valid schemes: ws://, wss://, http://, https://
    /// Default: ws://127.0.0.1:9944
    pub url: String,

    /// Additional chain URLs for multi-chain setup
    ///
    /// Env: SAS_SUBSTRATE_MULTI_CHAIN_URL
    /// Format: JSON array of objects with "url" and "type" fields
    /// Example: '[{"url":"ws://polkadot:9944","type":"relay"}]'
    /// Default: []
    pub multi_chain_urls: Vec<ChainUrl>,
}

impl SubstrateConfig {
    pub(crate) fn validate(&self) -> Result<(), ConfigError> {
        // Validate primary URL
        if self.url.is_empty() {
            return Err(ConfigError::ValidateError(
                "Substrate URL cannot be empty".to_string(),
            ));
        }

        Self::validate_url(&self.url)?;

        // Validate multi-chain URLs
        let mut seen_urls = std::collections::HashSet::new();
        seen_urls.insert(self.url.clone());

        for chain_url in &self.multi_chain_urls {
            // Validate URL format
            Self::validate_url(&chain_url.url)?;

            // Check for duplicates
            if !seen_urls.insert(chain_url.url.clone()) {
                return Err(ConfigError::ValidateError(format!(
                    "Duplicate URL found in multi-chain configuration: {}",
                    chain_url.url
                )));
            }
        }

        Ok(())
    }

    /// Validate a single URL
    fn validate_url(url_str: &str) -> Result<(), ConfigError> {
        if url_str.is_empty() {
            return Err(ConfigError::ValidateError(
                "URL cannot be empty".to_string(),
            ));
        }

        // Parse URL to check format
        let parsed = url::Url::parse(url_str)
            .map_err(|e| ConfigError::ValidateError(format!("Invalid URL '{}': {}", url_str, e)))?;

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
            multi_chain_urls: vec![],
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
            multi_chain_urls: vec![],
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_invalid_url_format() {
        let config = SubstrateConfig {
            url: "not-a-valid-url".to_string(),
            multi_chain_urls: vec![],
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_invalid_scheme() {
        let config = SubstrateConfig {
            url: "ftp://localhost:9944".to_string(),
            multi_chain_urls: vec![],
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_valid_ws_url() {
        let config = SubstrateConfig {
            url: "ws://localhost:9944".to_string(),
            multi_chain_urls: vec![],
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_valid_wss_url() {
        let config = SubstrateConfig {
            url: "wss://polkadot.api.io".to_string(),
            multi_chain_urls: vec![],
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_valid_http_url() {
        let config = SubstrateConfig {
            url: "http://localhost:9933".to_string(),
            multi_chain_urls: vec![],
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_valid_https_url() {
        let config = SubstrateConfig {
            url: "https://rpc.polkadot.io".to_string(),
            multi_chain_urls: vec![],
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_multi_chain_valid() {
        let config = SubstrateConfig {
            url: "ws://localhost:9944".to_string(),
            multi_chain_urls: vec![
                ChainUrl {
                    url: "ws://polkadot:9944".to_string(),
                    chain_type: ChainType::Relay,
                },
                ChainUrl {
                    url: "ws://asset-hub:9944".to_string(),
                    chain_type: ChainType::AssetHub,
                },
            ],
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_multi_chain_duplicate_url() {
        let config = SubstrateConfig {
            url: "ws://localhost:9944".to_string(),
            multi_chain_urls: vec![ChainUrl {
                url: "ws://localhost:9944".to_string(),
                chain_type: ChainType::Relay,
            }],
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_multi_chain_invalid_url() {
        let config = SubstrateConfig {
            url: "ws://localhost:9944".to_string(),
            multi_chain_urls: vec![ChainUrl {
                url: "invalid-url".to_string(),
                chain_type: ChainType::Relay,
            }],
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_chain_type_deserialization() {
        let json = r#"{"url":"ws://test:9944","type":"relay"}"#;
        let chain_url: ChainUrl = serde_json::from_str(json).unwrap();
        assert_eq!(chain_url.chain_type, ChainType::Relay);

        let json = r#"{"url":"ws://test:9944","type":"assethub"}"#;
        let chain_url: ChainUrl = serde_json::from_str(json).unwrap();
        assert_eq!(chain_url.chain_type, ChainType::AssetHub);

        let json = r#"{"url":"ws://test:9944","type":"parachain"}"#;
        let chain_url: ChainUrl = serde_json::from_str(json).unwrap();
        assert_eq!(chain_url.chain_type, ChainType::Parachain);
    }
}
