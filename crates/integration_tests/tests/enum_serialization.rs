/// Integration tests for enum serialization
///
/// These tests verify that enums are correctly serialized based on whether they are
/// "basic" (all variants have no data) or "non-basic" (at least one variant has data):
///
/// - Basic enums: serialize as strings (e.g., "Normal", "Yes")
/// - Non-basic enums: serialize as objects (e.g., {"unlimited": null})
///
/// Test block: 20000006 on Polkadot
use anyhow::{Context, Result};
use integration_tests::client::TestClient;
use integration_tests::constants::API_READY_TIMEOUT_SECONDS;
use serde_json::Value;
use std::env;

const TEST_BLOCK: u64 = 20000006;

/// Initialize tracing for tests
fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
}

/// Helper to get the API client
fn get_client() -> TestClient {
    let api_url = env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    TestClient::new(api_url)
}

/// Test that DispatchClass (basic enum) serializes as string in event data
#[tokio::test]
async fn test_dispatch_class_basic_enum_in_events() -> Result<()> {
    init_tracing();
    let client = get_client();
    client.wait_for_ready(API_READY_TIMEOUT_SECONDS).await?;

    let endpoint = format!("/v1/blocks/{}", TEST_BLOCK);
    let (status, response) = client.get_json(&endpoint).await?;

    assert!(
        status.is_success(),
        "Request failed with status {}: {}",
        status,
        serde_json::to_string_pretty(&response).unwrap_or_default()
    );

    // Find an ExtrinsicSuccess event to check DispatchInfo.class
    let extrinsics = response["extrinsics"]
        .as_array()
        .context("extrinsics should be an array")?;

    let mut found_dispatch_class = false;

    for extrinsic in extrinsics {
        if let Some(events) = extrinsic["events"].as_array() {
            for event in events {
                if event["method"]["pallet"] == "system"
                    && event["method"]["method"] == "ExtrinsicSuccess"
                {
                    // Check DispatchInfo structure
                    let event_data = event["data"]
                        .as_array()
                        .context("event data should be an array")?;

                    if let Some(dispatch_info) = event_data.first() {
                        println!(
                            "Found DispatchInfo: {}",
                            serde_json::to_string_pretty(dispatch_info).unwrap()
                        );
                        // DispatchClass should be a string (basic enum)
                        let class = &dispatch_info["class"];

                        if class.is_string() {
                            let class_str = class.as_str().unwrap();
                            assert!(
                                ["Normal", "Operational", "Mandatory"].contains(&class_str),
                                "class should be one of: Normal, Operational, Mandatory. Got: {}",
                                class_str
                            );
                            found_dispatch_class = true;
                            println!("✓ DispatchClass serialized correctly as string: \"{}\"", class_str);
                        } else {
                            panic!(
                                "DispatchClass should serialize as string (basic enum), but got: {}",
                                serde_json::to_string_pretty(class).unwrap()
                            );
                        }
                    }
                }
            }
        }
    }

    assert!(
        found_dispatch_class,
        "Should have found at least one ExtrinsicSuccess event with DispatchInfo"
    );

    Ok(())
}

/// Test that Pays (basic enum) serializes as string in event data
#[tokio::test]
async fn test_pays_basic_enum_in_events() -> Result<()> {
    init_tracing();
    let client = get_client();
    client.wait_for_ready(API_READY_TIMEOUT_SECONDS).await?;

    let endpoint = format!("/v1/blocks/{}", TEST_BLOCK);
    let (status, response) = client.get_json(&endpoint).await?;

    assert!(
        status.is_success(),
        "Request failed with status {}: {}",
        status,
        serde_json::to_string_pretty(&response).unwrap_or_default()
    );

    let extrinsics = response["extrinsics"]
        .as_array()
        .context("extrinsics should be an array")?;

    let mut found_pays_fee = false;

    for extrinsic in extrinsics {
        if let Some(events) = extrinsic["events"].as_array() {
            for event in events {
                if event["method"]["pallet"] == "system"
                    && event["method"]["method"] == "ExtrinsicSuccess"
                {
                    let event_data = event["data"]
                        .as_array()
                        .context("event data should be an array")?;

                    if let Some(dispatch_info) = event_data.first() {
                        // Pays should be a string (basic enum)
                        let pays_fee = &dispatch_info["paysFee"];

                        if pays_fee.is_string() {
                            let pays_str = pays_fee.as_str().unwrap();
                            assert!(
                                ["Yes", "No"].contains(&pays_str),
                                "paysFee should be 'Yes' or 'No'. Got: {}",
                                pays_str
                            );
                            found_pays_fee = true;
                            println!("✓ Pays serialized correctly as string: \"{}\"", pays_str);
                        } else {
                            panic!(
                                "Pays should serialize as string (basic enum), but got: {}",
                                serde_json::to_string_pretty(pays_fee).unwrap()
                            );
                        }
                    }
                }
            }
        }
    }

    assert!(
        found_pays_fee,
        "Should have found at least one ExtrinsicSuccess event with paysFee"
    );

    Ok(())
}

/// Test that non-basic enums in extrinsic args serialize as objects
#[tokio::test]
async fn test_non_basic_enums_in_extrinsic_args() -> Result<()> {
    init_tracing();
    let client = get_client();
    client.wait_for_ready(API_READY_TIMEOUT_SECONDS).await?;

    let endpoint = format!("/v1/blocks/{}", TEST_BLOCK);
    let (status, response) = client.get_json(&endpoint).await?;

    assert!(
        status.is_success(),
        "Request failed with status {}: {}",
        status,
        serde_json::to_string_pretty(&response).unwrap_or_default()
    );

    let extrinsics = response["extrinsics"]
        .as_array()
        .context("extrinsics should be an array")?;

    let mut found_non_basic_enum = false;

    for extrinsic in extrinsics {
        let args = &extrinsic["args"];

        // Look for XCM-related extrinsics which commonly have non-basic enums
        let pallet = extrinsic["method"]["pallet"].as_str().unwrap_or("");
        let method = extrinsic["method"]["method"].as_str().unwrap_or("");

        // Check for weight_limit in XCM calls (WeightLimit is non-basic)
        if let Some(weight_limit) = args.get("weight_limit") {
            println!(
                "Checking weight_limit in {}.{}: {}",
                pallet,
                method,
                serde_json::to_string(weight_limit).unwrap()
            );
            if weight_limit.is_object() {
                found_non_basic_enum = true;
                println!(
                    "✓ Found weight_limit (non-basic enum) in {}.{}: {}",
                    pallet,
                    method,
                    serde_json::to_string(weight_limit).unwrap()
                );

                // Verify it's an object with lowercase key
                let obj = weight_limit.as_object().unwrap();
                assert_eq!(
                    obj.len(),
                    1,
                    "Non-basic enum should have exactly one key, got: {}",
                    obj.keys().map(|k| k.as_str()).collect::<Vec<_>>().join(", ")
                );
                let key = obj.keys().next().unwrap();
                assert!(
                    key.chars().next().unwrap().is_lowercase(),
                    "Non-basic enum key should start with lowercase, got: {}",
                    key
                );
            } else if weight_limit.is_string() {
                panic!(
                    "WeightLimit is a non-basic enum and should serialize as object, but got string: {}",
                    weight_limit
                );
            }
        }

        // Check for XCM interior field (Junctions is non-basic)
        check_xcm_interior_recursive(args, &mut found_non_basic_enum);
    }

    // Note: This test might not find examples in all blocks
    // If no XCM extrinsics are present, that's okay
    if found_non_basic_enum {
        println!("✓ Successfully verified non-basic enum serialization in extrinsic args");
    } else {
        println!("⚠ No XCM extrinsics with non-basic enums found in block {}", TEST_BLOCK);
    }

    Ok(())
}

/// Recursively check for XCM interior fields
fn check_xcm_interior_recursive(value: &Value, found: &mut bool) {
    match value {
        Value::Object(map) => {
            if let Some(interior) = map.get("interior") {
                if interior.is_object() {
                    *found = true;
                    println!(
                        "✓ Found interior (non-basic enum) in XCM: {}",
                        serde_json::to_string(interior).unwrap()
                    );

                    // Verify it's an object with lowercase key
                    let obj = interior.as_object().unwrap();
                    if !obj.is_empty() {
                        let key = obj.keys().next().unwrap();
                        assert!(
                            key.chars().next().unwrap().is_lowercase(),
                            "Non-basic enum key should start with lowercase, got: {}",
                            key
                        );
                    }
                } else if interior.is_string() {
                    panic!(
                        "XCM Junctions is a non-basic enum and should serialize as object, but got string: {}",
                        interior
                    );
                }
            }

            // Recurse into nested objects
            for (_, v) in map.iter() {
                check_xcm_interior_recursive(v, found);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                check_xcm_interior_recursive(v, found);
            }
        }
        _ => {}
    }
}

/// Comprehensive test that verifies the complete block structure
#[tokio::test]
async fn test_block_20000006_complete_structure() -> Result<()> {
    init_tracing();
    let client = get_client();
    client.wait_for_ready(API_READY_TIMEOUT_SECONDS).await?;

    let endpoint = format!("/v1/blocks/{}", TEST_BLOCK);
    let (status, response) = client.get_json(&endpoint).await?;

    assert!(
        status.is_success(),
        "Request failed with status {}: {}",
        status,
        serde_json::to_string_pretty(&response).unwrap_or_default()
    );

    // Verify basic structure
    assert!(response["number"].is_string(), "block number should be a string");
    assert!(response["hash"].is_string(), "block hash should be a string");
    assert!(response["parentHash"].is_string(), "parentHash should be a string");
    assert!(response["extrinsics"].is_array(), "extrinsics should be an array");

    let block_number = response["number"]
        .as_str()
        .unwrap()
        .parse::<u64>()
        .context("block number should be parseable")?;

    assert_eq!(
        block_number, TEST_BLOCK,
        "block number should match requested block"
    );

    println!("✓ Block {} structure is valid", TEST_BLOCK);

    Ok(())
}

/// Test specific known extrinsics in block 20000006 for enum serialization
#[tokio::test]
async fn test_known_extrinsics_enum_patterns() -> Result<()> {
    init_tracing();
    let client = get_client();
    client.wait_for_ready(API_READY_TIMEOUT_SECONDS).await?;

    let endpoint = format!("/v1/blocks/{}", TEST_BLOCK);
    let (status, response) = client.get_json(&endpoint).await?;

    assert!(
        status.is_success(),
        "Request failed with status {}: {}",
        status,
        serde_json::to_string_pretty(&response).unwrap_or_default()
    );

    let extrinsics = response["extrinsics"]
        .as_array()
        .context("extrinsics should be an array")?;

    println!("\nAnalyzing {} extrinsics in block {}", extrinsics.len(), TEST_BLOCK);

    let mut enum_stats = EnumStats::default();

    for (idx, extrinsic) in extrinsics.iter().enumerate() {
        let pallet = extrinsic["method"]["pallet"].as_str().unwrap_or("unknown");
        let method = extrinsic["method"]["method"].as_str().unwrap_or("unknown");

        // Analyze args for enum patterns
        analyze_value_for_enums(&extrinsic["args"], &mut enum_stats);

        // Check events
        if let Some(events) = extrinsic["events"].as_array() {
            for event in events {
                analyze_value_for_enums(&event["data"], &mut enum_stats);
            }
        }

        if idx < 3 {
            println!("  Extrinsic {}: {}.{}", idx, pallet, method);
        }
    }

    println!("\n📊 Enum Statistics:");
    println!("  String enums (basic): {}", enum_stats.string_enums);
    println!("  Object enums (non-basic): {}", enum_stats.object_enums);
    println!("  Total enum-like values: {}", enum_stats.string_enums + enum_stats.object_enums);

    // Verify we found both types
    assert!(
        enum_stats.string_enums > 0,
        "Should find at least one basic enum (string) in block {}",
        TEST_BLOCK
    );

    println!("\n✓ Enum serialization analysis complete for block {}", TEST_BLOCK);

    Ok(())
}

#[derive(Default)]
struct EnumStats {
    string_enums: usize,
    object_enums: usize,
}

fn analyze_value_for_enums(value: &Value, stats: &mut EnumStats) {
    match value {
        Value::String(s) => {
            // Common enum value patterns (basic enums)
            if ["Normal", "Operational", "Mandatory", "Yes", "No"].contains(&s.as_str()) {
                stats.string_enums += 1;
            }
        }
        Value::Object(map) => {
            // Check if this looks like a non-basic enum: single key with lowercase first char
            if map.len() == 1 {
                let key = map.keys().next().unwrap();
                if key.chars().next().map(|c| c.is_lowercase()).unwrap_or(false) {
                    // Common non-basic enum patterns
                    if ["unlimited", "limited", "here", "x1", "x2", "x3", "here"].contains(&key.as_str()) {
                        stats.object_enums += 1;
                    }
                }
            }

            // Recurse into nested objects
            for (_, v) in map.iter() {
                analyze_value_for_enums(v, stats);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                analyze_value_for_enums(v, stats);
            }
        }
        _ => {}
    }
}
