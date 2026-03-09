// Copyright (C) 2026 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: GPL-3.0-or-later

use integration_tests::TestClient;

#[tokio::test]
async fn test_capabilities_endpoint() {
    let base_url = std::env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".into());
    let client = TestClient::new(base_url);

    if client.wait_for_ready(5).await.is_err() {
        println!("Skipping test: server not available");
        return;
    }

    let (status, json) = client
        .get_json("/v1/capabilities")
        .await
        .expect("request failed");

    assert!(status.is_success(), "Expected 200, got {}", status);
    assert!(
        json.get("chain").is_some(),
        "Response should have chain field"
    );
    assert!(
        json.get("pallets").is_some(),
        "Response should have pallets field"
    );

    let pallets = json["pallets"].as_array().expect("pallets should be array");
    assert!(!pallets.is_empty(), "Should have at least one pallet");

    let pallet_names: Vec<&str> = pallets.iter().filter_map(|p| p.as_str()).collect();
    assert!(
        pallet_names.contains(&"System"),
        "Should have System pallet"
    );

    println!("✓ Capabilities test passed with {} pallets", pallets.len());
}
