use serde::Deserialize;
use std::collections::HashMap;
use thiserror::Error;

/// Error type for fee configuration operations
#[derive(Debug, Error)]
pub enum FeeConfigError {
    #[error("Failed to parse fee config JSON: {0}")]
    JsonParseError(#[from] serde_json::Error),

    #[error("Chain '{0}' not found in fee configuration")]
    ChainNotFound(String),
}

/// Fee calculation configuration for a single chain
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChainFeeConfig {
    /// Minimum runtime spec version that supports fee calculation
    /// Below this version, fee calculation is not available
    pub min_calc_fee_runtime: u32,

    /// Highest known runtime spec version that does NOT have queryFeeDetails
    /// None means the status is unknown and needs to be discovered at runtime
    pub query_fee_details_unavailable: Option<u32>,

    /// Lowest known runtime spec version that DOES have queryFeeDetails
    /// None means the status is unknown and needs to be discovered at runtime
    pub query_fee_details_available: Option<u32>,
}

impl ChainFeeConfig {
    /// Check if fee calculation is supported for a given runtime version
    pub fn supports_fee_calculation(&self, spec_version: u32) -> bool {
        spec_version >= self.min_calc_fee_runtime
    }

    /// Check if queryFeeDetails is known to be available for a given runtime version
    /// Returns:
    /// - Some(true) if known to be available
    /// - Some(false) if known to be unavailable
    /// - None if status is unknown and needs to be discovered
    pub fn query_fee_details_status(&self, spec_version: u32) -> Option<bool> {
        match (
            self.query_fee_details_unavailable,
            self.query_fee_details_available,
        ) {
            (Some(unavail), Some(avail)) => {
                if spec_version <= unavail {
                    Some(false)
                } else if spec_version >= avail {
                    Some(true)
                } else {
                    // Between the known bounds - should not happen normally
                    None
                }
            }
            // If either bound is unknown, we can't determine status statically
            _ => None,
        }
    }
}

/// Collection of fee configurations for all supported chains
#[derive(Debug, Clone)]
pub struct ChainFeeConfigs {
    configs: HashMap<String, ChainFeeConfig>,
}

impl ChainFeeConfigs {
    /// Load fee configurations from the embedded JSON
    pub fn load() -> Result<Self, FeeConfigError> {
        // Embed the JSON config at compile time
        const CONFIG_JSON: &str = include_str!("chain_fee_config.json");

        let configs: HashMap<String, ChainFeeConfig> = serde_json::from_str(CONFIG_JSON)?;

        Ok(Self { configs })
    }

    /// Get the fee configuration for a specific chain by spec_name
    pub fn get(&self, spec_name: &str) -> Option<&ChainFeeConfig> {
        if let Some(config) = self.configs.get(spec_name) {
            return Some(config);
        }

        let lowercase = spec_name.to_lowercase();
        if let Some(config) = self.configs.get(&lowercase) {
            return Some(config);
        }

        let mapped_name = match lowercase.as_str() {
            "statemint" => "asset-hub-polkadot",
            "statemine" => "asset-hub-kusama",
            "westmint" => "asset-hub-westend",
            _ => return None,
        };

        self.configs.get(mapped_name)
    }

    /// Get the fee configuration for a chain, returning an error if not found
    pub fn get_or_error(&self, spec_name: &str) -> Result<&ChainFeeConfig, FeeConfigError> {
        self.get(spec_name)
            .ok_or_else(|| FeeConfigError::ChainNotFound(spec_name.to_string()))
    }

    /// List all configured chain names
    pub fn chain_names(&self) -> Vec<&str> {
        self.configs.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for ChainFeeConfigs {
    fn default() -> Self {
        Self::load().expect("Failed to load embedded fee config")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_config() {
        let configs = ChainFeeConfigs::load().unwrap();
        assert!(configs.get("polkadot").is_some());
        assert!(configs.get("kusama").is_some());
        assert!(configs.get("asset-hub-polkadot").is_some());
        assert!(configs.get("asset-hub-kusama").is_some());
    }

    #[test]
    fn test_polkadot_config() {
        let configs = ChainFeeConfigs::load().unwrap();
        let polkadot = configs.get("polkadot").unwrap();

        assert_eq!(polkadot.min_calc_fee_runtime, 0);
        assert_eq!(polkadot.query_fee_details_unavailable, Some(27));
        assert_eq!(polkadot.query_fee_details_available, Some(28));

        // Test fee calculation support
        assert!(polkadot.supports_fee_calculation(0));
        assert!(polkadot.supports_fee_calculation(100));

        // Test queryFeeDetails status
        assert_eq!(polkadot.query_fee_details_status(27), Some(false));
        assert_eq!(polkadot.query_fee_details_status(28), Some(true));
        assert_eq!(polkadot.query_fee_details_status(100), Some(true));
    }

    #[test]
    fn test_kusama_config() {
        let configs = ChainFeeConfigs::load().unwrap();
        let kusama = configs.get("kusama").unwrap();

        assert_eq!(kusama.min_calc_fee_runtime, 1058);
        assert_eq!(kusama.query_fee_details_unavailable, Some(2027));
        assert_eq!(kusama.query_fee_details_available, Some(2028));

        // Test fee calculation support
        assert!(!kusama.supports_fee_calculation(1057));
        assert!(kusama.supports_fee_calculation(1058));
        assert!(kusama.supports_fee_calculation(2000));
    }

    #[test]
    fn test_asset_hub_config() {
        let configs = ChainFeeConfigs::load().unwrap();
        let asset_hub = configs.get("asset-hub-polkadot").unwrap();

        assert_eq!(asset_hub.min_calc_fee_runtime, 601);
        // queryFeeDetails status is unknown for asset hubs
        assert_eq!(asset_hub.query_fee_details_unavailable, None);
        assert_eq!(asset_hub.query_fee_details_available, None);

        // Status should be None (unknown)
        assert_eq!(asset_hub.query_fee_details_status(1000), None);
    }

    #[test]
    fn test_chain_name_mapping() {
        let configs = ChainFeeConfigs::load().unwrap();

        // Statemint should map to asset-hub-polkadot
        let statemint = configs.get("statemint").unwrap();
        let asset_hub_polkadot = configs.get("asset-hub-polkadot").unwrap();
        assert_eq!(
            statemint.min_calc_fee_runtime,
            asset_hub_polkadot.min_calc_fee_runtime
        );

        // Statemine should map to asset-hub-kusama
        let statemine = configs.get("statemine").unwrap();
        let asset_hub_kusama = configs.get("asset-hub-kusama").unwrap();
        assert_eq!(
            statemine.min_calc_fee_runtime,
            asset_hub_kusama.min_calc_fee_runtime
        );
    }

    #[test]
    fn test_unknown_chain() {
        let configs = ChainFeeConfigs::load().unwrap();
        assert!(configs.get("unknown-chain").is_none());
        assert!(configs.get_or_error("unknown-chain").is_err());
    }
}
