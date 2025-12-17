use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SubstrateError {
    #[error("Substrate URL cannot be empty")]
    UrlEmpty,

    #[error("Invalid URL '{url}': {source}")]
    UrlParseError {
        url: String,
        #[source]
        source: url::ParseError,
    },

    #[error("Invalid URL scheme '{scheme}'. Must be ws://, wss://, http://, or https://")]
    InvalidUrlScheme { scheme: String },

    #[error("Duplicate URL found in multi-chain configuration: {url}")]
    DuplicateUrl { url: String },
}

/// Known relay chains in the ecosystem
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnownRelayChain {
    Polkadot,
    Kusama,
    Westend,
    Rococo,
    Paseo,
}

impl KnownRelayChain {
    /// Get the spec_name for this relay chain
    pub fn spec_name(&self) -> &'static str {
        match self {
            Self::Polkadot => "polkadot",
            Self::Kusama => "kusama",
            Self::Westend => "westend",
            Self::Rococo => "rococo",
            Self::Paseo => "paseo",
        }
    }

    /// Parse a relay chain from its spec_name (case-insensitive)
    pub fn from_spec_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "polkadot" => Some(Self::Polkadot),
            "kusama" => Some(Self::Kusama),
            "westend" => Some(Self::Westend),
            "rococo" => Some(Self::Rococo),
            "paseo" => Some(Self::Paseo),
            _ => None,
        }
    }
}

/// Known Asset Hub chains in the ecosystem
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnownAssetHub {
    Polkadot,
    Kusama,
    Westend,
    Paseo,
}

impl KnownAssetHub {
    /// Get the current spec_name for this Asset Hub
    pub fn spec_name(&self) -> &'static str {
        match self {
            Self::Polkadot => "asset-hub-polkadot",
            Self::Kusama => "asset-hub-kusama",
            Self::Westend => "asset-hub-westend",
            Self::Paseo => "asset-hub-paseo",
        }
    }

    /// Get the legacy spec_name for this Asset Hub
    pub fn legacy_spec_name(&self) -> &'static str {
        match self {
            Self::Polkadot => "statemint",
            Self::Kusama => "statemine",
            Self::Westend => "westmint",
            // AssetHub paseo has also been named as `asset-hub-paseo`
            Self::Paseo => "asset-hub-paseo",
        }
    }

    /// Parse an Asset Hub from its spec_name (handles both legacy and current names, case-insensitive)
    pub fn from_spec_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            // Current names
            "asset-hub-polkadot" => Some(Self::Polkadot),
            "asset-hub-kusama" => Some(Self::Kusama),
            "asset-hub-westend" => Some(Self::Westend),
            "asset-hub-paseo" => Some(Self::Paseo),
            // Legacy names
            "statemint" => Some(Self::Polkadot),
            "statemine" => Some(Self::Kusama),
            "westmint" => Some(Self::Westend),
            _ => None,
        }
    }
}

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
    /// Determine chain type from runtime spec name
    pub fn from_spec_name(spec_name: &str) -> Self {
        let name_lower = spec_name.to_lowercase();

        match name_lower.as_str() {
            // Check for known Asset Hubs first (most specific)
            name if KnownAssetHub::from_spec_name(name).is_some() => Self::AssetHub,
            // Check for known relay chains
            name if KnownRelayChain::from_spec_name(name).is_some() => Self::Relay,
            // Check for asset-hub prefix patterns (future-proofing for unknown Asset Hubs)
            name if name.starts_with("asset-hub-") || name.contains("assethub") => Self::AssetHub,
            // Default to Parachain for everything else
            _ => Self::Parachain,
        }
    }

    /// Extract the specific relay chain if this is a Relay chain type
    pub fn as_relay_chain(&self, spec_name: &str) -> Option<KnownRelayChain> {
        match self {
            Self::Relay => KnownRelayChain::from_spec_name(spec_name),
            _ => None,
        }
    }

    /// Extract the specific Asset Hub if this is an AssetHub chain type
    pub fn as_asset_hub(&self, spec_name: &str) -> Option<KnownAssetHub> {
        match self {
            Self::AssetHub => KnownAssetHub::from_spec_name(spec_name),
            _ => None,
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

    /// Initial delay in milliseconds for reconnection backoff
    ///
    /// Env: SAS_SUBSTRATE_RECONNECT_INITIAL_DELAY_MS
    /// Default: 100
    pub reconnect_initial_delay_ms: u64,

    /// Maximum delay in milliseconds for reconnection backoff
    ///
    /// Env: SAS_SUBSTRATE_RECONNECT_MAX_DELAY_MS
    /// Default: 60000 (60 seconds)
    pub reconnect_max_delay_ms: u64,

    /// Request timeout in milliseconds for RPC calls
    ///
    /// Env: SAS_SUBSTRATE_RECONNECT_REQUEST_TIMEOUT_MS
    /// Default: 60000 (60 seconds)
    pub reconnect_request_timeout_ms: u64,
}

impl SubstrateConfig {
    pub(crate) fn validate(&self) -> Result<(), SubstrateError> {
        // Validate primary URL
        if self.url.is_empty() {
            return Err(SubstrateError::UrlEmpty);
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
                return Err(SubstrateError::DuplicateUrl {
                    url: chain_url.url.clone(),
                });
            }
        }

        Ok(())
    }

    /// Validate a single URL
    fn validate_url(url_str: &str) -> Result<(), SubstrateError> {
        if url_str.is_empty() {
            return Err(SubstrateError::UrlEmpty);
        }

        // Parse URL to check format
        let parsed = url::Url::parse(url_str).map_err(|source| SubstrateError::UrlParseError {
            url: url_str.to_string(),
            source,
        })?;

        // Check scheme
        match parsed.scheme() {
            "ws" | "wss" | "http" | "https" => Ok(()),
            scheme => Err(SubstrateError::InvalidUrlScheme {
                scheme: scheme.to_string(),
            }),
        }
    }
}

impl Default for SubstrateConfig {
    fn default() -> Self {
        Self {
            url: "ws://127.0.0.1:9944".to_string(),
            multi_chain_urls: vec![],
            reconnect_initial_delay_ms: 100,
            reconnect_max_delay_ms: 60000,
            reconnect_request_timeout_ms: 60000,
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
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_invalid_url_format() {
        let config = SubstrateConfig {
            url: "not-a-valid-url".to_string(),
            multi_chain_urls: vec![],
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_invalid_scheme() {
        let config = SubstrateConfig {
            url: "ftp://localhost:9944".to_string(),
            multi_chain_urls: vec![],
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_valid_ws_url() {
        let config = SubstrateConfig {
            url: "ws://localhost:9944".to_string(),
            multi_chain_urls: vec![],
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_valid_wss_url() {
        let config = SubstrateConfig {
            url: "wss://polkadot.api.io".to_string(),
            multi_chain_urls: vec![],
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_valid_http_url() {
        let config = SubstrateConfig {
            url: "http://localhost:9933".to_string(),
            multi_chain_urls: vec![],
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_valid_https_url() {
        let config = SubstrateConfig {
            url: "https://rpc.polkadot.io".to_string(),
            multi_chain_urls: vec![],
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
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
    fn test_known_relay_chain_spec_name() {
        assert_eq!(KnownRelayChain::Polkadot.spec_name(), "polkadot");
        assert_eq!(KnownRelayChain::Kusama.spec_name(), "kusama");
        assert_eq!(KnownRelayChain::Westend.spec_name(), "westend");
        assert_eq!(KnownRelayChain::Rococo.spec_name(), "rococo");
        assert_eq!(KnownRelayChain::Paseo.spec_name(), "paseo");
    }

    #[test]
    fn test_known_relay_chain_from_spec_name() {
        assert_eq!(
            KnownRelayChain::from_spec_name("polkadot"),
            Some(KnownRelayChain::Polkadot)
        );
        assert_eq!(
            KnownRelayChain::from_spec_name("kusama"),
            Some(KnownRelayChain::Kusama)
        );
        assert_eq!(
            KnownRelayChain::from_spec_name("westend"),
            Some(KnownRelayChain::Westend)
        );
        assert_eq!(
            KnownRelayChain::from_spec_name("rococo"),
            Some(KnownRelayChain::Rococo)
        );
        assert_eq!(
            KnownRelayChain::from_spec_name("paseo"),
            Some(KnownRelayChain::Paseo)
        );
        // Test case insensitivity
        assert_eq!(
            KnownRelayChain::from_spec_name("Polkadot"),
            Some(KnownRelayChain::Polkadot)
        );
        assert_eq!(
            KnownRelayChain::from_spec_name("KUSAMA"),
            Some(KnownRelayChain::Kusama)
        );
        // Test unknown
        assert_eq!(KnownRelayChain::from_spec_name("unknown"), None);
    }

    #[test]
    fn test_known_asset_hub_spec_name() {
        assert_eq!(KnownAssetHub::Polkadot.spec_name(), "asset-hub-polkadot");
        assert_eq!(KnownAssetHub::Kusama.spec_name(), "asset-hub-kusama");
        assert_eq!(KnownAssetHub::Westend.spec_name(), "asset-hub-westend");
    }

    #[test]
    fn test_known_asset_hub_legacy_spec_name() {
        assert_eq!(KnownAssetHub::Polkadot.legacy_spec_name(), "statemint");
        assert_eq!(KnownAssetHub::Kusama.legacy_spec_name(), "statemine");
        assert_eq!(KnownAssetHub::Westend.legacy_spec_name(), "westmint");
    }

    #[test]
    fn test_known_asset_hub_from_spec_name_current() {
        assert_eq!(
            KnownAssetHub::from_spec_name("asset-hub-polkadot"),
            Some(KnownAssetHub::Polkadot)
        );
        assert_eq!(
            KnownAssetHub::from_spec_name("asset-hub-kusama"),
            Some(KnownAssetHub::Kusama)
        );
        assert_eq!(
            KnownAssetHub::from_spec_name("asset-hub-westend"),
            Some(KnownAssetHub::Westend)
        );
        // Test case insensitivity
        assert_eq!(
            KnownAssetHub::from_spec_name("Asset-Hub-Polkadot"),
            Some(KnownAssetHub::Polkadot)
        );
    }

    #[test]
    fn test_known_asset_hub_from_spec_name_legacy() {
        assert_eq!(
            KnownAssetHub::from_spec_name("statemint"),
            Some(KnownAssetHub::Polkadot)
        );
        assert_eq!(
            KnownAssetHub::from_spec_name("statemine"),
            Some(KnownAssetHub::Kusama)
        );
        assert_eq!(
            KnownAssetHub::from_spec_name("westmint"),
            Some(KnownAssetHub::Westend)
        );
        // Test case insensitivity
        assert_eq!(
            KnownAssetHub::from_spec_name("Statemint"),
            Some(KnownAssetHub::Polkadot)
        );
        assert_eq!(
            KnownAssetHub::from_spec_name("WESTMINT"),
            Some(KnownAssetHub::Westend)
        );
        // Test unknown
        assert_eq!(KnownAssetHub::from_spec_name("unknown"), None);
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

    #[test]
    fn test_chain_type_as_relay_chain() {
        let chain_type = ChainType::from_spec_name("polkadot");
        assert_eq!(
            chain_type.as_relay_chain("polkadot"),
            Some(KnownRelayChain::Polkadot)
        );

        let chain_type = ChainType::from_spec_name("kusama");
        assert_eq!(
            chain_type.as_relay_chain("kusama"),
            Some(KnownRelayChain::Kusama)
        );

        // Non-relay chain returns None
        let chain_type = ChainType::from_spec_name("statemint");
        assert_eq!(chain_type.as_relay_chain("statemint"), None);

        let chain_type = ChainType::from_spec_name("acala");
        assert_eq!(chain_type.as_relay_chain("acala"), None);
    }

    #[test]
    fn test_chain_type_as_asset_hub() {
        let chain_type = ChainType::from_spec_name("statemint");
        assert_eq!(
            chain_type.as_asset_hub("statemint"),
            Some(KnownAssetHub::Polkadot)
        );

        let chain_type = ChainType::from_spec_name("asset-hub-kusama");
        assert_eq!(
            chain_type.as_asset_hub("asset-hub-kusama"),
            Some(KnownAssetHub::Kusama)
        );

        // Non-asset-hub chain returns None
        let chain_type = ChainType::from_spec_name("polkadot");
        assert_eq!(chain_type.as_asset_hub("polkadot"), None);

        let chain_type = ChainType::from_spec_name("acala");
        assert_eq!(chain_type.as_asset_hub("acala"), None);
    }
}
