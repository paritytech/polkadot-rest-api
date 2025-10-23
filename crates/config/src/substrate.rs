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

impl ChainType {
    /// Relay chain spec names
    pub const RELAY_CHAINS: &'static [&'static str] =
        &["polkadot", "kusama", "westend", "rococo", "paseo"];

    /// Asset Hub spec names (legacy and current)
    /// Legacy: statemint (Polkadot), statemine (Kusama), westmint (Westend)
    /// Current: asset-hub-polkadot, asset-hub-kusama, asset-hub-westend
    pub const ASSET_HUB_CHAINS: &'static [&'static str] = &[
        "statemint",
        "statemine",
        "westmint",
        "asset-hub-polkadot",
        "asset-hub-kusama",
        "asset-hub-westend",
    ];

    /// Determine chain type from runtime spec name
    pub fn from_spec_name(spec_name: &str) -> Self {
        let name_lower = spec_name.to_lowercase();

        match name_lower.as_str() {
            // Check exact matches for Asset Hub chains first (most specific)
            name if Self::ASSET_HUB_CHAINS.contains(&name) => Self::AssetHub,
            // Check exact matches for relay chains
            name if Self::RELAY_CHAINS.contains(&name) => Self::Relay,
            // Check for asset-hub prefix patterns (future-proofing)
            name if name.starts_with("asset-hub-") || name.contains("assethub") => Self::AssetHub,
            // Default to Parachain for everything else
            _ => Self::Parachain,
        }
    }
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

    #[test]
    fn test_chain_type_from_spec_name_relay() {
        assert_eq!(ChainType::from_spec_name("polkadot"), ChainType::Relay);
        assert_eq!(ChainType::from_spec_name("kusama"), ChainType::Relay);
        assert_eq!(ChainType::from_spec_name("westend"), ChainType::Relay);
        assert_eq!(ChainType::from_spec_name("rococo"), ChainType::Relay);
        assert_eq!(ChainType::from_spec_name("paseo"), ChainType::Relay);
        // Test case insensitivity
        assert_eq!(ChainType::from_spec_name("Polkadot"), ChainType::Relay);
        assert_eq!(ChainType::from_spec_name("KUSAMA"), ChainType::Relay);
    }

    #[test]
    fn test_chain_type_from_spec_name_asset_hub_legacy() {
        assert_eq!(ChainType::from_spec_name("statemint"), ChainType::AssetHub);
        assert_eq!(ChainType::from_spec_name("statemine"), ChainType::AssetHub);
        assert_eq!(ChainType::from_spec_name("westmint"), ChainType::AssetHub);
        // Test case insensitivity
        assert_eq!(ChainType::from_spec_name("Statemint"), ChainType::AssetHub);
        assert_eq!(ChainType::from_spec_name("WESTMINT"), ChainType::AssetHub);
    }

    #[test]
    fn test_chain_type_from_spec_name_asset_hub_current() {
        assert_eq!(
            ChainType::from_spec_name("asset-hub-polkadot"),
            ChainType::AssetHub
        );
        assert_eq!(
            ChainType::from_spec_name("asset-hub-kusama"),
            ChainType::AssetHub
        );
        assert_eq!(
            ChainType::from_spec_name("asset-hub-westend"),
            ChainType::AssetHub
        );
        // Test case insensitivity
        assert_eq!(
            ChainType::from_spec_name("Asset-Hub-Polkadot"),
            ChainType::AssetHub
        );
    }

    #[test]
    fn test_chain_type_from_spec_name_parachain() {
        assert_eq!(ChainType::from_spec_name("acala"), ChainType::Parachain);
        assert_eq!(ChainType::from_spec_name("moonbeam"), ChainType::Parachain);
        assert_eq!(ChainType::from_spec_name("astar"), ChainType::Parachain);
        assert_eq!(
            ChainType::from_spec_name("unknown-chain"),
            ChainType::Parachain
        );
    }
}
