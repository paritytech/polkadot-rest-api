use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ChainConfigError {
    #[error("Failed to parse chain config JSON: {0}")]
    JsonParseError(#[from] serde_json::Error),

    #[error("Chain '{0}' not found in configuration")]
    ChainNotFound(String),
}

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
        Ok(Self::from_str(&s))
    }
}

impl Hasher {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().replace('_', "-").as_str() {
            "keccak-256" | "keccak256" => Hasher::Keccak256,
            _ => Hasher::Blake2_256,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainConfig {
    #[serde(default = "default_finalizes")]
    pub finalizes: bool,

    #[serde(default)]
    pub min_calc_fee_runtime: u32,

    #[serde(default)]
    pub query_fee_details_unavailable: Option<u32>,

    #[serde(default)]
    pub query_fee_details_available: Option<u32>,

    #[serde(default = "default_block_number_bytes")]
    pub block_number_bytes: usize,

    #[serde(default)]
    pub hasher: Hasher,

    /// Legacy types: "polkadot" or "none"
    #[serde(default = "default_legacy_types")]
    pub legacy_types: String,

    #[serde(default)]
    pub supports_ahm: bool,
}

impl Default for ChainConfig {
    fn default() -> Self {
        Self {
            finalizes: default_finalizes(),
            min_calc_fee_runtime: 0,
            query_fee_details_unavailable: None,
            query_fee_details_available: None,
            block_number_bytes: default_block_number_bytes(),
            hasher: Hasher::default(),
            legacy_types: default_legacy_types(),
            supports_ahm: false,
        }
    }
}

fn default_finalizes() -> bool {
    true
}

fn default_block_number_bytes() -> usize {
    4 // u32
}

fn default_legacy_types() -> String {
    "none".to_string()
}

/// Status of queryFeeDetails RPC availability for a given spec version
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryFeeDetailsStatus {
    /// queryFeeDetails is available at this spec version
    Available,
    /// queryFeeDetails is not available at this spec version
    Unavailable,
    /// Availability is unknown and needs to be discovered via RPC
    Unknown,
}

impl ChainConfig {
    pub fn supports_fee_calculation(&self, spec_version: u32) -> bool {
        spec_version >= self.min_calc_fee_runtime
    }

    pub fn query_fee_details_status(&self, spec_version: u32) -> QueryFeeDetailsStatus {
        match (
            self.query_fee_details_unavailable,
            self.query_fee_details_available,
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
            _ => QueryFeeDetailsStatus::Unknown,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChainConfigs {
    configs: HashMap<String, ChainConfig>,
}

impl ChainConfigs {
    pub fn load() -> Result<Self, ChainConfigError> {
        const CONFIG_JSON: &str = include_str!("chain_config.json");
        let configs: HashMap<String, ChainConfig> = serde_json::from_str(CONFIG_JSON)?;
        Ok(Self { configs })
    }

    pub fn get(&self, spec_name: &str) -> Option<&ChainConfig> {
        self.configs
            .get(spec_name)
            .or_else(|| self.configs.get(&spec_name.to_lowercase()))
    }

    pub fn get_or_error(&self, spec_name: &str) -> Result<&ChainConfig, ChainConfigError> {
        self.get(spec_name)
            .ok_or_else(|| ChainConfigError::ChainNotFound(spec_name.to_string()))
    }

    pub fn chain_names(&self) -> Vec<&str> {
        self.configs.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for ChainConfigs {
    fn default() -> Self {
        Self::load().expect("Failed to load embedded chain configurations")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_chain_configs() {
        let configs = ChainConfigs::load().unwrap();
        assert!(!configs.chain_names().is_empty());
    }

    #[test]
    fn test_get_polkadot_config() {
        let configs = ChainConfigs::load().unwrap();
        let polkadot = configs.get("polkadot").expect("Polkadot config should exist");
        
        assert_eq!(polkadot.finalizes, true);
        assert_eq!(polkadot.block_number_bytes, 4);
        assert_eq!(polkadot.hasher, Hasher::Blake2_256);
    }

    #[test]
    fn test_get_case_insensitive() {
        let configs = ChainConfigs::load().unwrap();
        assert!(configs.get("Polkadot").is_some());
        assert!(configs.get("POLKADOT").is_some());
    }

    #[test]
    fn test_fee_calculation_support() {
        let config = ChainConfig {
            finalizes: true,
            min_calc_fee_runtime: 100,
            query_fee_details_unavailable: Some(27),
            query_fee_details_available: Some(28),
            block_number_bytes: 4,
            hasher: Hasher::Blake2_256,
            supports_ahm: false,
            legacy_types: "none".to_string(),
        };

        assert!(!config.supports_fee_calculation(99));
        assert!(config.supports_fee_calculation(100));
        assert!(config.supports_fee_calculation(1000));
    }

    #[test]
    fn test_query_fee_details_status_available() {
        let config = ChainConfig {
            finalizes: true,
            min_calc_fee_runtime: 0,
            query_fee_details_unavailable: Some(27),
            query_fee_details_available: Some(28),
            block_number_bytes: 4,
            hasher: Hasher::Blake2_256,
            supports_ahm: false,
            legacy_types: "none".to_string(),
        };

        // Before unavailable threshold
        assert_eq!(
            config.query_fee_details_status(26),
            QueryFeeDetailsStatus::Unavailable
        );
        // At unavailable threshold
        assert_eq!(
            config.query_fee_details_status(27),
            QueryFeeDetailsStatus::Unavailable
        );
        // At available threshold
        assert_eq!(
            config.query_fee_details_status(28),
            QueryFeeDetailsStatus::Available
        );
        // After available threshold
        assert_eq!(
            config.query_fee_details_status(100),
            QueryFeeDetailsStatus::Available
        );
    }

    #[test]
    fn test_query_fee_details_status_unknown() {
        // Config with null values - status is always unknown
        let config = ChainConfig {
            finalizes: true,
            min_calc_fee_runtime: 0,
            query_fee_details_unavailable: None,
            query_fee_details_available: None,
            block_number_bytes: 4,
            hasher: Hasher::Blake2_256,
            supports_ahm: false,
            legacy_types: "none".to_string(),
        };

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
    fn test_query_fee_details_status_gap() {
        // Config with a gap between unavailable and available
        let config = ChainConfig {
            finalizes: true,
            min_calc_fee_runtime: 0,
            query_fee_details_unavailable: Some(100),
            query_fee_details_available: Some(200),
            block_number_bytes: 4,
            hasher: Hasher::Blake2_256,
            supports_ahm: false,
            legacy_types: "none".to_string(),
        };

        // In the gap - status is unknown
        assert_eq!(
            config.query_fee_details_status(150),
            QueryFeeDetailsStatus::Unknown
        );
    }

    #[test]
    fn test_all_configured_chains_exist() {
        let configs = ChainConfigs::load().unwrap();
        let chain_names = configs.chain_names();

        // Verify all expected chains are present
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

        for chain in expected_chains {
            assert!(
                chain_names.contains(&chain),
                "Chain '{}' should be in config",
                chain
            );
        }
    }

    #[test]
    fn test_polkadot_config_values() {
        let configs = ChainConfigs::load().unwrap();
        let polkadot = configs.get("polkadot").unwrap();

        assert_eq!(polkadot.finalizes, true);
        assert_eq!(polkadot.min_calc_fee_runtime, 0);
        assert_eq!(polkadot.query_fee_details_unavailable, Some(27));
        assert_eq!(polkadot.query_fee_details_available, Some(28));
        assert_eq!(polkadot.block_number_bytes, 4);
        assert_eq!(polkadot.hasher, Hasher::Blake2_256);
        assert_eq!(polkadot.legacy_types, "polkadot");
        assert_eq!(polkadot.supports_ahm, true);
    }

    #[test]
    fn test_kusama_config_values() {
        let configs = ChainConfigs::load().unwrap();
        let kusama = configs.get("kusama").unwrap();

        assert_eq!(kusama.finalizes, true);
        assert_eq!(kusama.min_calc_fee_runtime, 1058);
        assert_eq!(kusama.query_fee_details_unavailable, Some(2027));
        assert_eq!(kusama.query_fee_details_available, Some(2028));
        assert_eq!(kusama.block_number_bytes, 4);
        assert_eq!(kusama.hasher, Hasher::Blake2_256);
    }

    #[test]
    fn test_asset_hub_chains_have_null_query_fee_details() {
        let configs = ChainConfigs::load().unwrap();
        
        // Asset hubs should have null for query fee details (unknown status)
        let asset_hubs = vec![
            "statemint",
            "statemine",
            "westmint",
            "asset-hub-polkadot",
            "asset-hub-kusama",
            "asset-hub-westend",
        ];

        for chain_name in asset_hubs {
            let config = configs.get(chain_name)
                .expect(&format!("Chain '{}' should exist", chain_name));
            
            assert_eq!(
                config.query_fee_details_unavailable, None,
                "Chain '{}' should have null query_fee_details_unavailable",
                chain_name
            );
            assert_eq!(
                config.query_fee_details_available, None,
                "Chain '{}' should have null query_fee_details_available",
                chain_name
            );
        }
    }

    #[test]
    fn test_relay_chains_have_polkadot_legacy_types() {
        let configs = ChainConfigs::load().unwrap();

        // Relay chains should use "polkadot" legacy types
        let relay_chains = ["polkadot", "kusama", "westend"];

        for chain_name in relay_chains {
            let config = configs
                .get(chain_name)
                .expect(&format!("Chain '{}' should exist", chain_name));

            assert_eq!(
                config.legacy_types, "polkadot",
                "Relay chain '{}' should use 'polkadot' legacy types",
                chain_name
            );
        }
    }

    #[test]
    fn test_asset_hubs_have_no_legacy_types() {
        let configs = ChainConfigs::load().unwrap();

        // Asset hubs don't need legacy types (metadata V14+)
        let asset_hubs = [
            "statemint",
            "statemine",
            "westmint",
            "asset-hub-polkadot",
            "asset-hub-kusama",
            "asset-hub-westend",
        ];

        for chain_name in asset_hubs {
            let config = configs
                .get(chain_name)
                .expect(&format!("Chain '{}' should exist", chain_name));

            assert_eq!(
                config.legacy_types, "none",
                "Asset hub '{}' should use 'none' legacy types",
                chain_name
            );
        }
    }

    #[test]
    fn test_get_or_error_success() {
        let configs = ChainConfigs::load().unwrap();
        let result = configs.get_or_error("polkadot");
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_or_error_not_found() {
        let configs = ChainConfigs::load().unwrap();
        let result = configs.get_or_error("nonexistent-chain");
        assert!(result.is_err());
        
        match result {
            Err(ChainConfigError::ChainNotFound(chain)) => {
                assert_eq!(chain, "nonexistent-chain");
            }
            _ => panic!("Expected ChainNotFound error"),
        }
    }

    #[test]
    fn test_get_unknown_chain_returns_none() {
        let configs = ChainConfigs::load().unwrap();
        assert!(configs.get("unknown-chain").is_none());
        assert!(configs.get("").is_none());
    }

    #[test]
    fn test_default_values() {
        // Test that default functions work correctly
        assert_eq!(default_finalizes(), true);
        assert_eq!(default_block_number_bytes(), 4);
        assert_eq!(Hasher::default(), Hasher::Blake2_256);
    }

    #[test]
    fn test_all_chains_have_required_fields() {
        let configs = ChainConfigs::load().unwrap();
        
        for chain_name in configs.chain_names() {
            let config = configs.get(chain_name).unwrap();
            
            // All chains should have these basic properties
            assert!(config.block_number_bytes > 0, 
                "Chain '{}' should have positive block_number_bytes", chain_name);
            // Hasher is always valid (type-safe enum)
            // Note: finalizes can be true or false, both are valid
            // Note: min_calc_fee_runtime can be 0 or higher, both are valid
        }
    }

    #[test]
    fn test_supports_fee_calculation_at_boundary() {
        let config = ChainConfig {
            finalizes: true,
            min_calc_fee_runtime: 1000,
            query_fee_details_unavailable: None,
            query_fee_details_available: None,
            block_number_bytes: 4,
            hasher: Hasher::Blake2_256,
            supports_ahm: false,
            legacy_types: "none".to_string(),
        };

        // Below minimum
        assert!(!config.supports_fee_calculation(999));
        // At minimum (boundary)
        assert!(config.supports_fee_calculation(1000));
        // Above minimum
        assert!(config.supports_fee_calculation(1001));
    }

    #[test]
    fn test_westend_has_unknown_query_fee_details() {
        let configs = ChainConfigs::load().unwrap();
        let westend = configs.get("westend").unwrap();

        // Westend has null values, so status should always be Unknown
        assert_eq!(
            westend.query_fee_details_status(0),
            QueryFeeDetailsStatus::Unknown
        );
        assert_eq!(
            westend.query_fee_details_status(1000),
            QueryFeeDetailsStatus::Unknown
        );
    }

    #[test]
    fn test_chain_config_error_display() {
        // Test error message formatting
        let error = ChainConfigError::ChainNotFound("test-chain".to_string());
        assert_eq!(
            format!("{}", error),
            "Chain 'test-chain' not found in configuration"
        );
    }

    #[test]
    fn test_chain_config_error_from_json() {
        // Test that invalid JSON produces JsonParseError
        let invalid_json = r#"{ invalid json }"#;
        let result: Result<HashMap<String, ChainConfig>, serde_json::Error> =
            serde_json::from_str(invalid_json);

        assert!(result.is_err());
    }

    #[test]
    fn test_default_chain_configs() {
        // Test that Default trait works correctly
        let configs = ChainConfigs::default();
        assert!(!configs.chain_names().is_empty());
        assert!(configs.get("polkadot").is_some());
    }

    #[test]
    fn test_chain_config_clone() {
        // Test that ChainConfig can be cloned
        let config = ChainConfig {
            finalizes: true,
            min_calc_fee_runtime: 100,
            query_fee_details_unavailable: Some(27),
            query_fee_details_available: Some(28),
            block_number_bytes: 4,
            hasher: Hasher::Blake2_256,
            supports_ahm: false,
            legacy_types: "none".to_string(),
        };

        let cloned = config.clone();
        assert_eq!(config.finalizes, cloned.finalizes);
        assert_eq!(config.min_calc_fee_runtime, cloned.min_calc_fee_runtime);
        assert_eq!(
            config.query_fee_details_unavailable,
            cloned.query_fee_details_unavailable
        );
        assert_eq!(
            config.query_fee_details_available,
            cloned.query_fee_details_available
        );
        assert_eq!(config.block_number_bytes, cloned.block_number_bytes);
        assert_eq!(config.hasher, cloned.hasher);
    }

    #[test]
    fn test_query_fee_details_status_clone_copy() {
        // Test that QueryFeeDetailsStatus implements Copy
        let status = QueryFeeDetailsStatus::Available;
        let copied = status; // This should work because of Copy
        assert_eq!(status, copied);
    }

    #[test]
    fn test_chain_configs_clone() {
        // Test that ChainConfigs can be cloned
        let configs = ChainConfigs::load().unwrap();
        let cloned = configs.clone();

        assert_eq!(configs.chain_names().len(), cloned.chain_names().len());
    }

    #[test]
    fn test_all_chains_have_standard_block_number_bytes() {
        let configs = ChainConfigs::load().unwrap();

        // All current chains use 4 bytes (u32) for block numbers
        for chain_name in configs.chain_names() {
            let config = configs.get(chain_name).unwrap();
            assert_eq!(
                config.block_number_bytes, 4,
                "Chain '{}' should use 4 bytes for block numbers",
                chain_name
            );
        }
    }

    #[test]
    fn test_all_chains_use_blake2_256() {
        let configs = ChainConfigs::load().unwrap();

        // All current Substrate chains use blake2-256
        for chain_name in configs.chain_names() {
            let config = configs.get(chain_name).unwrap();
            assert_eq!(
                config.hasher, Hasher::Blake2_256,
                "Chain '{}' should use blake2-256 hasher",
                chain_name
            );
        }
    }

    #[test]
    fn test_all_chains_finalize() {
        let configs = ChainConfigs::load().unwrap();

        // All current chains should finalize blocks
        for chain_name in configs.chain_names() {
            let config = configs.get(chain_name).unwrap();
            assert!(
                config.finalizes,
                "Chain '{}' should finalize blocks",
                chain_name
            );
        }
    }
}
