// Integration tests for chain config validation
use config::{ChainConfig, ChainConfigs, ChainType, Hasher};

#[test]
fn test_all_expected_chains_exist() {
    let configs = ChainConfigs::default();

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
        assert!(
            configs.get(chain_name).is_some(),
            "Chain '{}' should exist in embedded config",
            chain_name
        );
    }
}

#[test]
fn test_relay_chains_have_correct_config() {
    let configs = ChainConfigs::default();

    for chain_name in &["polkadot", "kusama", "westend"] {
        let config = configs.get(chain_name).unwrap();

        assert_eq!(
            config.chain_type,
            ChainType::Relay,
            "{} should be relay chain",
            chain_name
        );
        assert_eq!(
            config.legacy_types, "polkadot",
            "{} should use polkadot legacy types",
            chain_name
        );
        assert!(config.finalizes, "{} should finalize", chain_name);
        assert_eq!(
            config.relay_chain, None,
            "{} should not have relay_chain",
            chain_name
        );
        assert_eq!(
            config.para_id, None,
            "{} should not have para_id",
            chain_name
        );
    }
}

#[test]
fn test_asset_hubs_have_correct_config() {
    let configs = ChainConfigs::default();

    let asset_hub_configs = vec![
        ("asset-hub-polkadot", "polkadot", 1000),
        ("asset-hub-kusama", "kusama", 1000),
        ("asset-hub-westend", "westend", 1000),
        ("statemint", "polkadot", 1000),
        ("statemine", "kusama", 1000),
        ("westmint", "westend", 1000),
    ];

    for (chain_name, expected_relay, expected_para_id) in asset_hub_configs {
        let config = configs.get(chain_name).unwrap();

        assert_eq!(
            config.chain_type,
            ChainType::AssetHub,
            "{} should be asset hub",
            chain_name
        );
        assert_eq!(
            config.legacy_types, "none",
            "{} should use no legacy types",
            chain_name
        );
        assert_eq!(
            config.relay_chain,
            Some(expected_relay.to_string()),
            "{} should have relay_chain = {}",
            chain_name,
            expected_relay
        );
        assert_eq!(
            config.para_id,
            Some(expected_para_id),
            "{} should have para_id = {}",
            chain_name,
            expected_para_id
        );
    }
}

#[test]
fn test_all_chains_have_valid_block_number_bytes() {
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
        assert!(
            config.block_number_bytes >= 1 && config.block_number_bytes <= 8,
            "{} has invalid block_number_bytes: {}",
            chain_name,
            config.block_number_bytes
        );
    }
}

#[test]
fn test_all_chains_have_valid_hashers() {
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
        assert!(
            config.hasher == Hasher::Blake2_256 || config.hasher == Hasher::Keccak256,
            "{} has invalid hasher: {:?}",
            chain_name,
            config.hasher
        );
    }
}

#[test]
fn test_spec_versions_is_optional() {
    let configs = ChainConfigs::default();

    // Most chains should have spec_versions set to null (None)
    let polkadot = configs.get("polkadot").unwrap();
    assert!(
        polkadot.spec_versions.is_none(),
        "polkadot should have spec_versions = None (optimization not required)"
    );
}

#[test]
fn test_chain_config_is_case_insensitive() {
    let configs = ChainConfigs::default();

    // Should be able to look up with different cases
    assert!(configs.get("Polkadot").is_some());
    assert!(configs.get("polkadot").is_some());
    assert!(configs.get("POLKADOT").is_some());

    assert!(configs.get("Kusama").is_some());
    assert!(configs.get("kusama").is_some());
    assert!(configs.get("KUSAMA").is_some());
}

#[test]
fn test_unknown_chain_returns_none() {
    let configs = ChainConfigs::default();

    assert!(configs.get("nonexistent-chain").is_none());
    assert!(configs.get("unknown").is_none());
    assert!(configs.get("").is_none());
}

#[test]
fn test_fee_config_values_are_reasonable() {
    let configs = ChainConfigs::default();

    let polkadot = configs.get("polkadot").unwrap();
    assert_eq!(
        polkadot.min_calc_fee_runtime, 0,
        "Polkadot min_calc_fee_runtime should be 0"
    );

    // QueryFeeDetails thresholds should be set for Polkadot
    assert_eq!(polkadot.query_fee_details_unavailable_at, Some(27));
    assert_eq!(polkadot.query_fee_details_available_at, Some(28));
}

#[test]
fn test_config_dual_connection_support() {
    let chain_config = ChainConfig::default();
    let relay_config = ChainConfig::default();

    // Test single-chain config
    let single_config = config::Config::single_chain(chain_config.clone());
    assert!(!single_config.has_relay_chain());
    assert!(single_config.rc.is_none());

    // Test dual-chain config
    let dual_config = config::Config::with_relay_chain(chain_config, relay_config);
    assert!(dual_config.has_relay_chain());
    assert!(dual_config.rc.is_some());
}
