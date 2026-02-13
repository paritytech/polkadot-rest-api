// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Polkadot REST API",
        version = "0.1.0",
        description = "High-performance Rust REST API for Substrate/Polkadot blockchain data. Drop-in replacement for substrate-api-sidecar.",
        license(name = "GPL-3.0-or-later"),
        contact(url = "https://github.com/paritytech/polkadot-rest-api")
    ),
    servers(
        (url = "http://localhost:8080", description = "Localhost")
    ),
    tags(
        (name = "health", description = "Health check"),
        (name = "node", description = "Connected node information"),
        (name = "version", description = "API version"),
        (name = "blocks", description = "Block queries and extrinsic data"),
        (name = "accounts", description = "Account balance, staking, and proxy information"),
        (name = "pallets", description = "Runtime pallet metadata, storage, constants, events, errors"),
        (name = "runtime", description = "Runtime specification, metadata, and code"),
        (name = "transaction", description = "Transaction submission, fee estimation, and construction material"),
        (name = "coretime", description = "Coretime system information"),
        (name = "paras", description = "Parachain inclusion data"),
        (name = "ahm", description = "Asset Hub Migration information"),
        (name = "capabilities", description = "API capabilities and chain pallets"),
        (name = "rc", description = "Relay chain endpoints (available on parachains only)"),
    ),
    paths(
        // Health & System
        crate::handlers::health::get_health::get_health,
        crate::handlers::version::get_version::get_version,
        crate::handlers::capabilities::get_capabilities,
        crate::handlers::ahm::get_ahm_info::ahm_info,
        // Node
        crate::handlers::node::get_node_version::get_node_version,
        crate::handlers::node::get_node_network::get_node_network,
        crate::handlers::node::get_node_transaction_pool::get_node_transaction_pool,
        // Blocks
        crate::handlers::blocks::get_block::get_block,
        crate::handlers::blocks::get_block_head::get_block_head,
        crate::handlers::blocks::get_blocks_head_header::get_blocks_head_header,
        crate::handlers::blocks::get_block_header::get_block_header,
        crate::handlers::blocks::get_blocks::get_blocks,
        crate::handlers::blocks::get_block_extrinsics_raw::get_block_extrinsics_raw,
        crate::handlers::blocks::get_extrinsic::get_extrinsic,
        crate::handlers::blocks::get_block_para_inclusions::get_block_para_inclusions,
        // Accounts
        crate::handlers::accounts::get_balance_info::get_balance_info,
        crate::handlers::accounts::get_asset_balances::get_asset_balances,
        crate::handlers::accounts::get_asset_approvals::get_asset_approvals,
        crate::handlers::accounts::get_pool_asset_balances::get_pool_asset_balances,
        crate::handlers::accounts::get_pool_asset_approvals::get_pool_asset_approvals,
        crate::handlers::accounts::get_staking_info::get_staking_info,
        crate::handlers::accounts::get_staking_payouts::get_staking_payouts,
        crate::handlers::accounts::get_vesting_info::get_vesting_info,
        crate::handlers::accounts::get_proxy_info::get_proxy_info,
        crate::handlers::accounts::get_convert::get_convert,
        crate::handlers::accounts::get_validate::get_validate,
        crate::handlers::accounts::get_compare::get_compare,
        crate::handlers::accounts::get_foreign_asset_balances::get_foreign_asset_balances,
        // Pallets
        crate::handlers::pallets::storage::get_pallets_storage,
        crate::handlers::pallets::storage::get_pallets_storage_item,
        crate::handlers::pallets::consts::pallets_constants,
        crate::handlers::pallets::consts::pallets_constant_item,
        crate::handlers::pallets::errors::get_pallet_errors,
        crate::handlers::pallets::errors::get_pallet_error_item,
        crate::handlers::pallets::events::get_pallet_events,
        crate::handlers::pallets::events::get_pallet_event_item,
        crate::handlers::pallets::dispatchables::get_pallets_dispatchables,
        crate::handlers::pallets::dispatchables::get_pallet_dispatchable_item,
        crate::handlers::pallets::staking_progress::pallets_staking_progress,
        crate::handlers::pallets::staking_validators::pallets_staking_validators,
        crate::handlers::pallets::nomination_pools::pallets_nomination_pools_info,
        crate::handlers::pallets::nomination_pools::pallets_nomination_pools_pool,
        crate::handlers::pallets::assets::pallets_assets_asset_info,
        crate::handlers::pallets::pool_assets::pallets_pool_assets_asset_info,
        crate::handlers::pallets::foreign_assets::pallets_foreign_assets,
        crate::handlers::pallets::asset_conversion::get_liquidity_pools,
        crate::handlers::pallets::asset_conversion::get_next_available_id,
        crate::handlers::pallets::on_going_referenda::pallets_on_going_referenda,
        // Runtime
        crate::handlers::runtime::get_spec::runtime_spec,
        crate::handlers::runtime::get_code::runtime_code,
        crate::handlers::runtime::get_metadata::runtime_metadata,
        crate::handlers::runtime::get_metadata::runtime_metadata_versions,
        crate::handlers::runtime::get_metadata::runtime_metadata_versioned,
        // Transaction
        crate::handlers::transaction::submit::submit,
        crate::handlers::transaction::dry_run::dry_run,
        crate::handlers::transaction::fee_estimate::fee_estimate,
        crate::handlers::transaction::material::material,
        crate::handlers::transaction::material::material_versioned,
        crate::handlers::transaction::metadata_blob::metadata_blob,
        // Coretime
        crate::handlers::coretime::info::coretime_info,
        crate::handlers::coretime::overview::coretime_overview,
        crate::handlers::coretime::leases::coretime_leases,
        crate::handlers::coretime::regions::coretime_regions,
        crate::handlers::coretime::renewals::coretime_renewals,
        crate::handlers::coretime::reservations::coretime_reservations,
        // Paras
        crate::handlers::paras::paras_inclusion::get_paras_inclusion,
        // RC - Blocks
        crate::handlers::rc::blocks::get_head::get_rc_blocks_head,
        crate::handlers::rc::blocks::get_head_header::get_rc_blocks_head_header,
        crate::handlers::rc::blocks::get_rc_block::get_rc_block,
        crate::handlers::rc::blocks::get_block_header::get_rc_block_header,
        crate::handlers::rc::blocks::get_rc_blocks::get_rc_blocks,
        crate::handlers::rc::blocks::get_rc_block_extrinsics_raw::get_rc_block_extrinsics_raw,
        crate::handlers::rc::blocks::get_rc_extrinsic::get_rc_extrinsic,
        crate::handlers::rc::blocks::get_rc_block_para_inclusions::get_rc_block_para_inclusions,
        // RC - Accounts
        crate::handlers::rc::accounts::get_balance_info::get_balance_info,
        crate::handlers::rc::accounts::get_proxy_info::get_proxy_info,
        crate::handlers::rc::accounts::get_staking_info::get_staking_info,
        crate::handlers::rc::accounts::get_staking_payouts::get_staking_payouts,
        crate::handlers::rc::accounts::get_vesting_info::get_vesting_info,
        // RC - Node
        crate::handlers::rc::node::get_rc_node_network::get_rc_node_network,
        crate::handlers::rc::node::get_rc_node_version::get_rc_node_version,
        crate::handlers::rc::node::get_rc_node_transaction_pool::get_rc_node_transaction_pool,
        // RC - Runtime
        crate::handlers::rc::runtime::get_rc_runtime_spec::get_rc_runtime_spec,
        crate::handlers::rc::runtime::get_rc_runtime_code::get_rc_runtime_code,
        crate::handlers::rc::runtime::get_rc_runtime_metadata::get_rc_runtime_metadata,
        crate::handlers::rc::runtime::get_rc_runtime_metadata::get_rc_runtime_metadata_versions,
        crate::handlers::rc::runtime::get_rc_runtime_metadata::get_rc_runtime_metadata_versioned,
        // RC - Pallets
        crate::handlers::pallets::staking_progress::rc_pallets_staking_progress,
        crate::handlers::pallets::staking_validators::rc_pallets_staking_validators,
        crate::handlers::pallets::consts::rc_pallets_constants,
        crate::handlers::pallets::consts::rc_pallets_constant_item,
        crate::handlers::pallets::dispatchables::rc_pallets_dispatchables,
        crate::handlers::pallets::dispatchables::rc_pallet_dispatchable_item,
        crate::handlers::pallets::errors::rc_pallet_errors,
        crate::handlers::pallets::errors::rc_pallet_error_item,
        crate::handlers::pallets::events::rc_pallet_events,
        crate::handlers::pallets::events::rc_pallet_event_item,
        crate::handlers::pallets::storage::rc_get_pallets_storage,
        crate::handlers::pallets::storage::rc_get_pallets_storage_item,
        crate::handlers::pallets::on_going_referenda::rc_pallets_on_going_referenda,
        // RC - Transaction
        crate::handlers::transaction::submit::submit_rc,
        crate::handlers::transaction::dry_run::dry_run_rc,
        crate::handlers::transaction::fee_estimate::fee_estimate_rc,
        crate::handlers::transaction::material::material_rc,
        crate::handlers::transaction::material::material_versioned_rc,
        crate::handlers::transaction::metadata_blob::metadata_blob_rc,
    ),
)]
pub struct ApiDoc;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::{self, RouteRegistry};
    use config::ChainType;
    use std::collections::BTreeSet;
    use utoipa::OpenApi;

    /// Normalize path parameters: replace `{anything}` with `{}` for structural comparison.
    /// This allows matching even when parameter names differ between Axum routes and utoipa
    /// annotations (e.g., `:pallet_id` vs `{palletId}`).
    fn normalize_path(path: &str) -> String {
        let mut result = String::new();
        let mut in_brace = false;
        for c in path.chars() {
            match c {
                '{' => {
                    in_brace = true;
                    result.push('{');
                }
                '}' => {
                    in_brace = false;
                    result.push('}');
                }
                _ if !in_brace => result.push(c),
                _ => {} // skip param name chars inside braces
            }
        }
        result
    }

    /// Build the full route registry as `create_app` would, using `ChainType::Coretime`
    /// for maximum route coverage (includes standard, coretime-specific, parachain, and
    /// relay-chain-proxy routes).
    fn build_full_registry() -> RouteRegistry {
        let registry = RouteRegistry::new();
        let chain_type = ChainType::Coretime;

        let _ = routes::accounts::accounts_routes(&registry);
        let _ = routes::ahm::routes(&registry);
        let _ = routes::blocks::blocks_routes(&registry);
        let _ = routes::capabilities::routes(&registry);
        let _ = routes::coretime::routes(&registry, &chain_type);
        let _ = routes::health::routes(&registry);
        let _ = routes::node::routes(&registry);
        let _ = routes::pallets::routes(&registry, &chain_type);
        let _ = routes::paras::routes(&registry, &chain_type);
        let _ = routes::rc::routes(&registry, &chain_type);
        let _ = routes::runtime::routes(&registry);
        let _ = routes::transaction::routes(&registry, &chain_type);
        let _ = routes::version::routes(&registry);

        registry
    }

    /// Verify that every registered route has a corresponding OpenAPI path and vice versa.
    /// This test catches:
    /// - New routes added without utoipa annotations (undocumented)
    /// - OpenAPI paths that don't correspond to any registered route (phantom docs)
    /// - Path mismatches between route registration and utoipa annotation
    #[test]
    fn openapi_paths_match_registered_routes() {
        let registry = build_full_registry();

        // Collect registered routes as "METHOD /normalized/path"
        let registered: BTreeSet<String> = registry
            .routes()
            .into_iter()
            .map(|r| format!("{} {}", r.method.to_uppercase(), normalize_path(&r.path)))
            .collect();

        // Collect OpenAPI spec paths as "METHOD /normalized/path"
        let spec = ApiDoc::openapi();
        let json_value = serde_json::to_value(&spec).expect("Failed to serialize OpenAPI spec");

        let mut openapi: BTreeSet<String> = BTreeSet::new();
        if let Some(paths) = json_value["paths"].as_object() {
            for (path, methods) in paths {
                if let Some(methods_obj) = methods.as_object() {
                    for method in methods_obj.keys() {
                        if matches!(method.as_str(), "get" | "post" | "put" | "delete" | "patch") {
                            openapi.insert(format!(
                                "{} {}",
                                method.to_uppercase(),
                                normalize_path(path)
                            ));
                        }
                    }
                }
            }
        }

        // Find differences
        let undocumented: Vec<&String> = registered.difference(&openapi).collect();
        let phantom: Vec<&String> = openapi.difference(&registered).collect();

        let mut errors = String::new();

        if !undocumented.is_empty() {
            errors.push_str(
                "\nRoutes registered but MISSING from OpenAPI spec \
                 (add #[utoipa::path] and register in openapi.rs):\n",
            );
            for route in &undocumented {
                errors.push_str(&format!("  - {}\n", route));
            }
        }

        if !phantom.is_empty() {
            errors.push_str(
                "\nRoutes in OpenAPI spec but NOT registered \
                 (stale path in openapi.rs or wrong path= in annotation):\n",
            );
            for route in &phantom {
                errors.push_str(&format!("  - {}\n", route));
            }
        }

        assert!(
            undocumented.is_empty() && phantom.is_empty(),
            "OpenAPI spec is out of sync with registered routes:\n{}",
            errors
        );
    }
}
