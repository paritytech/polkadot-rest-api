use crate::substrate::ChainType;
use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
use std::convert::Infallible;
use std::str::FromStr;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ChainConfigError {
    #[error("Failed to parse chain config JSON: {0}")]
    JsonParseError(#[from] serde_json::Error),

    #[error("Chain '{0}' not found in configuration")]
    ChainNotFound(String),
}

/// Hash function used by a chain
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Hasher {
    #[default]
    Blake2_256,
    Keccak256,
}

impl<'de> Deserialize<'de> for Hasher {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(s.parse().expect("invalid hasher string"))
    }
}

impl FromStr for Hasher {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let hasher = match s.to_lowercase().replace('_', "-").as_str() {
            "keccak-256" | "keccak256" => Hasher::Keccak256,
            _ => Hasher::Blake2_256,
        };
        Ok(hasher)
    }
}

impl std::fmt::Display for Hasher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Hasher::Blake2_256 => write!(f, "Blake2_256"),
            Hasher::Keccak256 => write!(f, "Keccak256"),
        }
    }
}

/// Chain-specific configuration
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainConfig {
    #[serde(default = "default_finalizes")]
    pub finalizes: bool,

    #[serde(default)]
    pub min_calc_fee_runtime: u32,

    #[serde(default)]
    pub query_fee_details_unavailable_at: Option<u32>,

    #[serde(default)]
    pub query_fee_details_available_at: Option<u32>,

    #[serde(default = "default_block_number_bytes")]
    pub block_number_bytes: usize,

    #[serde(default)]
    pub hasher: Hasher,

    #[serde(default = "default_legacy_types")]
    pub legacy_types: String,

    #[serde(default)]
    pub spec_versions: Option<crate::SpecVersionChanges>,

    #[serde(default)]
    pub chain_type: ChainType,

    #[serde(default)]
    pub relay_chain: Option<String>,

    #[serde(default)]
    pub para_id: Option<u32>,
}

impl Default for ChainConfig {
    fn default() -> Self {
        Self {
            finalizes: default_finalizes(),
            min_calc_fee_runtime: 0,
            query_fee_details_unavailable_at: None,
            query_fee_details_available_at: None,
            block_number_bytes: default_block_number_bytes(),
            hasher: Hasher::default(),
            legacy_types: default_legacy_types(),
            spec_versions: None,
            chain_type: ChainType::default(),
            relay_chain: None,
            para_id: None,
        }
    }
}

fn default_finalizes() -> bool {
    true
}

fn default_block_number_bytes() -> usize {
    4
}

fn default_legacy_types() -> String {
    "none".to_string()
}

/// QueryFeeDetails RPC availability status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryFeeDetailsStatus {
    Available,
    Unavailable,
    Unknown,
}

impl std::fmt::Display for QueryFeeDetailsStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QueryFeeDetailsStatus::Available => write!(f, "Available"),
            QueryFeeDetailsStatus::Unavailable => write!(f, "Unavailable"),
            QueryFeeDetailsStatus::Unknown => write!(f, "Unknown"),
        }
    }
}

impl ChainConfig {
    pub fn supports_fee_calculation(&self, spec_version: u32) -> bool {
        spec_version >= self.min_calc_fee_runtime
    }

    pub fn query_fee_details_status(&self, spec_version: u32) -> QueryFeeDetailsStatus {
        match (
            self.query_fee_details_unavailable_at,
            self.query_fee_details_available_at,
        ) {
            (Some(unavail), Some(avail)) => {
                if spec_version <= unavail {
                    QueryFeeDetailsStatus::Unavailable
                } else if spec_version >= avail {
                    QueryFeeDetailsStatus::Available
                } else {
                    QueryFeeDetailsStatus::Unknown
                }
            }
            (Some(unavail), None) => {
                if spec_version <= unavail {
                    QueryFeeDetailsStatus::Unavailable
                } else {
                    QueryFeeDetailsStatus::Unknown
                }
            }
            (None, Some(avail)) => {
                if spec_version >= avail {
                    QueryFeeDetailsStatus::Available
                } else {
                    QueryFeeDetailsStatus::Unknown
                }
            }
            (None, None) => QueryFeeDetailsStatus::Unknown,
        }
    }
}

/// Container for all chain configurations
#[derive(Debug, Clone)]
pub struct ChainConfigs {
    configs: HashMap<String, ChainConfig>,
}

impl Default for ChainConfigs {
    fn default() -> Self {
        Self::load_embedded()
    }
}

impl ChainConfigs {
    fn load_embedded() -> Self {
        const EMBEDDED_CONFIG: &str = include_str!("chain_config.json");
        Self::from_json_str(EMBEDDED_CONFIG).expect("Failed to parse embedded chain_config.json")
    }

    pub fn from_json_str(json: &str) -> Result<Self, ChainConfigError> {
        let raw: HashMap<String, ChainConfig> = serde_json::from_str(json)?;

        let configs: HashMap<String, ChainConfig> = raw
            .into_iter()
            .map(|(k, v)| (k.to_lowercase(), v))
            .collect();

        Ok(Self { configs })
    }

    pub fn get(&self, chain_name: &str) -> Option<&ChainConfig> {
        self.configs.get(&chain_name.to_lowercase())
    }

    /// Get all configured chain names
    pub fn chain_names(&self) -> Vec<String> {
        self.configs.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hasher_from_str() {
        assert_eq!("blake2-256".parse::<Hasher>().unwrap(), Hasher::Blake2_256);
        assert_eq!("Blake2_256".parse::<Hasher>().unwrap(), Hasher::Blake2_256);
        assert_eq!("keccak-256".parse::<Hasher>().unwrap(), Hasher::Keccak256);
        assert_eq!("keccak256".parse::<Hasher>().unwrap(), Hasher::Keccak256);
        assert_eq!("unknown".parse::<Hasher>().unwrap(), Hasher::Blake2_256);
    }

    #[test]
    fn test_chain_config_defaults() {
        let config = ChainConfig::default();
        assert!(config.finalizes);
        assert_eq!(config.min_calc_fee_runtime, 0);
        assert_eq!(config.block_number_bytes, 4);
        assert_eq!(config.hasher, Hasher::Blake2_256);
        assert_eq!(config.legacy_types, "none");
    }

    #[test]
    fn test_supports_fee_calculation() {
        let config = ChainConfig {
            min_calc_fee_runtime: 1000,
            ..Default::default()
        };

        assert!(!config.supports_fee_calculation(999));
        assert!(config.supports_fee_calculation(1000));
        assert!(config.supports_fee_calculation(1001));
    }

    #[test]
    fn test_query_fee_details_status() {
        let config = ChainConfig {
            query_fee_details_unavailable_at: Some(27),
            query_fee_details_available_at: Some(28),
            ..Default::default()
        };

        assert_eq!(
            config.query_fee_details_status(26),
            QueryFeeDetailsStatus::Unavailable
        );
        assert_eq!(
            config.query_fee_details_status(27),
            QueryFeeDetailsStatus::Unavailable
        );
        assert_eq!(
            config.query_fee_details_status(28),
            QueryFeeDetailsStatus::Available
        );
        assert_eq!(
            config.query_fee_details_status(29),
            QueryFeeDetailsStatus::Available
        );
    }

    #[test]
    fn test_chain_configs_from_json() {
        let json = r#"{
            "polkadot": {
                "finalizes": true,
                "minCalcFeeRuntime": 0,
                "queryFeeDetailsUnavailableAt": 27,
                "queryFeeDetailsAvailableAt": 28,
                "blockNumberBytes": 4,
                "hasher": "blake2-256",
                "legacyTypes": "polkadot",
                "chainType": "relay"
            },
            "asset-hub-polkadot": {
                "finalizes": true,
                "minCalcFeeRuntime": 601,
                "blockNumberBytes": 4,
                "hasher": "blake2-256",
                "legacyTypes": "none",
                "chainType": "assethub",
                "relayChain": "polkadot",
                "paraId": 1000
            }
        }"#;

        let configs = ChainConfigs::from_json_str(json).unwrap();

        let polkadot = configs.get("polkadot").unwrap();
        assert_eq!(polkadot.min_calc_fee_runtime, 0);
        assert_eq!(polkadot.hasher, Hasher::Blake2_256);
        assert_eq!(polkadot.legacy_types, "polkadot");

        let asset_hub = configs.get("asset-hub-polkadot").unwrap();
        assert_eq!(asset_hub.min_calc_fee_runtime, 601);
    }

    #[test]
    fn test_chain_configs_case_insensitive_lookup() {
        let json = r#"{"Polkadot": {"finalizes": true}}"#;
        let configs = ChainConfigs::from_json_str(json).unwrap();

        assert!(configs.get("Polkadot").is_some());
        assert!(configs.get("polkadot").is_some());
        assert!(configs.get("POLKADOT").is_some());
    }

    #[test]
    fn test_load_embedded_config() {
        let configs = ChainConfigs::default();

        // Verify we can load some expected chains
        assert!(configs.get("polkadot").is_some());
        assert!(configs.get("kusama").is_some());
        assert!(configs.get("westend").is_some());
    }

    #[test]
    fn test_all_embedded_chains_have_required_fields() {
        let configs = ChainConfigs::default();

        // Test all expected chains exist and have valid config
        let expected_chains = vec![
            "polkadot",
            "kusama",
            "westend",
            "statemint",
            "statemine",
            "westmint",
            "asset-hub-polkadot",
            "asset-hub-kusama",
            "asset-hub-westend",
        ];

        for chain_name in expected_chains {
            let config = configs
                .get(chain_name)
                .unwrap_or_else(|| panic!("Chain '{}' should exist in config", chain_name));

            // Verify reasonable defaults
            assert!(
                config.block_number_bytes > 0,
                "{}: block_number_bytes should be > 0",
                chain_name
            );
            assert!(
                config.block_number_bytes <= 8,
                "{}: block_number_bytes should be <= 8",
                chain_name
            );
        }
    }

    #[test]
    fn test_relay_chains_config() {
        let configs = ChainConfigs::default();

        for chain in &["polkadot", "kusama", "westend"] {
            let config = configs.get(chain).unwrap();
            assert_eq!(
                config.legacy_types, "polkadot",
                "{} should use polkadot legacy types",
                chain
            );
        }
    }

    #[test]
    fn test_asset_hubs_config() {
        let configs = ChainConfigs::default();

        let asset_hubs = vec![
            ("statemint", "Asset Hub Polkadot legacy"),
            ("statemine", "Asset Hub Kusama legacy"),
            ("westmint", "Asset Hub Westend legacy"),
            ("asset-hub-polkadot", "Asset Hub Polkadot current"),
            ("asset-hub-kusama", "Asset Hub Kusama current"),
            ("asset-hub-westend", "Asset Hub Westend current"),
        ];

        for (chain, description) in asset_hubs {
            let config = configs.get(chain).unwrap();
            assert_eq!(
                config.legacy_types, "none",
                "{} should use no legacy types",
                description
            );
        }
    }

    #[test]
    fn test_hasher_display() {
        assert_eq!(Hasher::Blake2_256.to_string(), "Blake2_256");
        assert_eq!(Hasher::Keccak256.to_string(), "Keccak256");
    }

    #[test]
    fn test_hasher_debug() {
        assert_eq!(format!("{:?}", Hasher::Blake2_256), "Blake2_256");
        assert_eq!(format!("{:?}", Hasher::Keccak256), "Keccak256");
    }

    #[test]
    fn test_query_fee_details_status_display() {
        assert_eq!(QueryFeeDetailsStatus::Available.to_string(), "Available");
        assert_eq!(
            QueryFeeDetailsStatus::Unavailable.to_string(),
            "Unavailable"
        );
        assert_eq!(QueryFeeDetailsStatus::Unknown.to_string(), "Unknown");
    }

    #[test]
    fn test_query_fee_details_no_thresholds() {
        let config = ChainConfig::default();

        // With no thresholds set, should always be Unknown
        assert_eq!(
            config.query_fee_details_status(0),
            QueryFeeDetailsStatus::Unknown
        );
        assert_eq!(
            config.query_fee_details_status(1000),
            QueryFeeDetailsStatus::Unknown
        );
    }

    #[test]
    fn test_query_fee_details_only_unavailable_threshold() {
        let config = ChainConfig {
            query_fee_details_unavailable_at: Some(100),
            ..Default::default()
        };

        // Should be Unavailable before threshold, Unknown after
        assert_eq!(
            config.query_fee_details_status(99),
            QueryFeeDetailsStatus::Unavailable
        );
        assert_eq!(
            config.query_fee_details_status(100),
            QueryFeeDetailsStatus::Unavailable
        );
        assert_eq!(
            config.query_fee_details_status(101),
            QueryFeeDetailsStatus::Unknown
        );
    }

    #[test]
    fn test_query_fee_details_only_available_threshold() {
        let config = ChainConfig {
            query_fee_details_available_at: Some(100),
            ..Default::default()
        };

        // Should be Unknown before threshold, Available after
        assert_eq!(
            config.query_fee_details_status(99),
            QueryFeeDetailsStatus::Unknown
        );
        assert_eq!(
            config.query_fee_details_status(100),
            QueryFeeDetailsStatus::Available
        );
        assert_eq!(
            config.query_fee_details_status(101),
            QueryFeeDetailsStatus::Available
        );
    }

    #[test]
    fn test_query_fee_details_unavailable_equals_available() {
        // Edge case where both thresholds are the same value.
        // Expect inclusive semantics: the unavailable check (<=) takes precedence.
        let config = ChainConfig {
            query_fee_details_unavailable_at: Some(27),
            query_fee_details_available_at: Some(27),
            ..Default::default()
        };

        // Below the threshold -> Unavailable
        assert_eq!(
            config.query_fee_details_status(26),
            QueryFeeDetailsStatus::Unavailable
        );

        // At the threshold -> Unavailable due to inclusive semantics
        assert_eq!(
            config.query_fee_details_status(27),
            QueryFeeDetailsStatus::Unavailable
        );

        // Above the threshold -> Available
        assert_eq!(
            config.query_fee_details_status(28),
            QueryFeeDetailsStatus::Available
        );
    }

    #[test]
    fn test_supports_fee_calculation_at_zero() {
        let config = ChainConfig {
            min_calc_fee_runtime: 0,
            ..Default::default()
        };

        // Should support fee calculation from block 0
        assert!(config.supports_fee_calculation(0));
        assert!(config.supports_fee_calculation(1));
    }

    #[test]
    fn test_supports_fee_calculation_high_threshold() {
        let config = ChainConfig {
            min_calc_fee_runtime: 1_000_000,
            ..Default::default()
        };

        assert!(!config.supports_fee_calculation(999_999));
        assert!(config.supports_fee_calculation(1_000_000));
    }

    #[test]
    fn test_chain_config_clone() {
        let config1 = ChainConfig {
            finalizes: false,
            min_calc_fee_runtime: 123,
            query_fee_details_unavailable_at: Some(10),
            query_fee_details_available_at: Some(20),
            block_number_bytes: 8,
            hasher: Hasher::Keccak256,
            legacy_types: "custom".to_string(),
            spec_versions: Default::default(),
            chain_type: ChainType::Relay,
            relay_chain: None,
            para_id: None,
        };

        let config2 = config1.clone();
        assert_eq!(config1.finalizes, config2.finalizes);
        assert_eq!(config1.min_calc_fee_runtime, config2.min_calc_fee_runtime);
        assert_eq!(config1.hasher, config2.hasher);
        assert_eq!(config1.legacy_types, config2.legacy_types);
    }

    #[test]
    fn test_chain_configs_get_or_default() {
        let configs = ChainConfigs::default();

        // Existing chain
        let _polkadot = configs.get("polkadot").unwrap();

        // Non-existing chain returns None
        assert!(configs.get("non-existent-chain").is_none());
    }

    #[test]
    fn test_invalid_json_returns_error() {
        let invalid_json = r#"{ invalid json }"#;
        let result = ChainConfigs::from_json_str(invalid_json);
        assert!(result.is_err());
    }

    #[test]
    fn test_empty_json_object() {
        let empty_json = r#"{}"#;
        let configs = ChainConfigs::from_json_str(empty_json).unwrap();
        assert!(configs.get("polkadot").is_none());
    }

    #[test]
    fn test_partial_config_uses_defaults() {
        let json = r#"{
            "test-chain": {
                "finalizes": false
            }
        }"#;

        let configs = ChainConfigs::from_json_str(json).unwrap();
        let config = configs.get("test-chain").unwrap();

        // Specified value
        assert!(!config.finalizes);

        // Should use defaults for unspecified fields
        assert_eq!(config.min_calc_fee_runtime, 0);
        assert_eq!(config.block_number_bytes, 4);
        assert_eq!(config.hasher, Hasher::Blake2_256);
        assert_eq!(config.legacy_types, "none");
    }

    #[test]
    fn test_hasher_variations() {
        // Test all case variations
        assert_eq!("blake2-256".parse::<Hasher>().unwrap(), Hasher::Blake2_256);
        assert_eq!("Blake2-256".parse::<Hasher>().unwrap(), Hasher::Blake2_256);
        assert_eq!("BLAKE2-256".parse::<Hasher>().unwrap(), Hasher::Blake2_256);
        assert_eq!("blake2_256".parse::<Hasher>().unwrap(), Hasher::Blake2_256);
        assert_eq!("Blake2_256".parse::<Hasher>().unwrap(), Hasher::Blake2_256);

        assert_eq!("keccak-256".parse::<Hasher>().unwrap(), Hasher::Keccak256);
        assert_eq!("Keccak-256".parse::<Hasher>().unwrap(), Hasher::Keccak256);
        assert_eq!("KECCAK-256".parse::<Hasher>().unwrap(), Hasher::Keccak256);
        assert_eq!("keccak256".parse::<Hasher>().unwrap(), Hasher::Keccak256);
        assert_eq!("Keccak256".parse::<Hasher>().unwrap(), Hasher::Keccak256);
    }

    #[test]
    fn test_polkadot_specific_config() {
        let configs = ChainConfigs::default();
        let polkadot = configs.get("polkadot").unwrap();

        assert!(polkadot.finalizes);
        assert_eq!(polkadot.min_calc_fee_runtime, 0);
        assert_eq!(polkadot.legacy_types, "polkadot");
        assert_eq!(polkadot.hasher, Hasher::Blake2_256);
    }

    #[test]
    fn test_kusama_specific_config() {
        let configs = ChainConfigs::default();
        let kusama = configs.get("kusama").unwrap();

        assert!(kusama.finalizes);
        assert_eq!(kusama.legacy_types, "polkadot"); // Uses polkadot legacy types
        assert_eq!(kusama.hasher, Hasher::Blake2_256);
    }

    #[test]
    fn test_block_number_bytes_range() {
        let configs = ChainConfigs::default();

        // All chains should have reasonable block_number_bytes (typically 4)
        for chain in &["polkadot", "kusama", "westend", "statemint", "statemine"] {
            let config = configs.get(chain).unwrap();
            assert!(
                config.block_number_bytes >= 4 && config.block_number_bytes <= 8,
                "{} has invalid block_number_bytes: {}",
                chain,
                config.block_number_bytes
            );
        }
    }

    #[test]
    fn test_spec_versions_is_optional() {
        let json = r#"{"test": {"specVersions": null}}"#;
        let configs = ChainConfigs::from_json_str(json).unwrap();
        let config = configs.get("test").unwrap();
        assert!(config.spec_versions.is_none());
    }

    #[test]
    fn test_spec_versions_when_present() {
        let json = r#"{
            "test": {
                "specVersions": {"changes": {"0": 1000, "1000": 1001}}
            }
        }"#;
        let configs = ChainConfigs::from_json_str(json).unwrap();
        let config = configs.get("test").unwrap();
        assert!(config.spec_versions.is_some());
        let spec_versions = config.spec_versions.as_ref().unwrap();
        assert_eq!(spec_versions.get_version_at_block(500), Some(1000));
        assert_eq!(spec_versions.get_version_at_block(1000), Some(1001));
    }

    #[test]
    fn test_parachain_topology_fields() {
        let configs = ChainConfigs::default();
        let ahp = configs.get("asset-hub-polkadot").unwrap();
        assert_eq!(ahp.chain_type, crate::substrate::ChainType::AssetHub);
        assert_eq!(ahp.relay_chain, Some("polkadot".to_string()));
        assert_eq!(ahp.para_id, Some(1000));
    }

    #[test]
    fn test_relay_chain_topology_fields() {
        let configs = ChainConfigs::default();
        let polkadot = configs.get("polkadot").unwrap();
        assert_eq!(polkadot.chain_type, crate::substrate::ChainType::Relay);
        assert_eq!(polkadot.relay_chain, None);
        assert_eq!(polkadot.para_id, None);
    }

    #[test]
    fn test_all_embedded_chains_have_valid_hashers() {
        let configs = ChainConfigs::default();
        let all_chains = vec![
            "polkadot",
            "kusama",
            "westend",
            "statemint",
            "statemine",
            "westmint",
            "asset-hub-polkadot",
            "asset-hub-kusama",
            "asset-hub-westend",
        ];

        for chain_name in all_chains {
            let config = configs.get(chain_name).unwrap();
            // Verify hasher is valid (should not panic)
            let _ = format!("{}", config.hasher);
            assert!(
                config.hasher == Hasher::Blake2_256 || config.hasher == Hasher::Keccak256,
                "{} has invalid hasher",
                chain_name
            );
        }
    }

    #[test]
    fn test_legacy_chain_names_exist() {
        let configs = ChainConfigs::default();
        assert!(configs.get("statemint").is_some(), "statemint should exist");
        assert!(configs.get("statemine").is_some(), "statemine should exist");
        assert!(configs.get("westmint").is_some(), "westmint should exist");
    }

    #[test]
    fn test_new_chain_names_exist() {
        let configs = ChainConfigs::default();
        assert!(
            configs.get("asset-hub-polkadot").is_some(),
            "asset-hub-polkadot should exist"
        );
        assert!(
            configs.get("asset-hub-kusama").is_some(),
            "asset-hub-kusama should exist"
        );
        assert!(
            configs.get("asset-hub-westend").is_some(),
            "asset-hub-westend should exist"
        );
    }
}
