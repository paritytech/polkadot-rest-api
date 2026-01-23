use anyhow::{Context, Result};
use integration_tests::{
    client::TestClient, constants::API_READY_TIMEOUT_SECONDS, utils::compare_json,
};
use serde_json::Value;
use std::env;
use std::fs;
use std::path::PathBuf;

#[tokio::test]
async fn test_blocks_range_valid() -> Result<()> {
    init_tracing();

    let api_url = env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let client = TestClient::new(api_url);

    client.wait_for_ready(API_READY_TIMEOUT_SECONDS).await?;

    let (status, json) = client.get_json("/v1/blocks?range=0-0").await?;
    assert!(
        status.is_success(),
        "GET /v1/blocks?range=0-0 should succeed"
    );
    assert!(json.is_array(), "Response should be an array");
    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 1, "Range 0-0 should return exactly one block");
    assert!(
        arr[0].get("number").is_some(),
        "Block should contain 'number'"
    );
    assert!(arr[0].get("hash").is_some(), "Block should contain 'hash'");

    let (status, json) = client.get_json("/v1/blocks?range=0-2").await?;
    assert!(
        status.is_success(),
        "GET /v1/blocks?range=0-2 should succeed"
    );
    assert!(json.is_array(), "Response should be an array");
    let arr = json.as_array().unwrap();
    assert!(
        arr.len() >= 1 && arr.len() <= 3,
        "Range 0-2 should return between 1 and 3 blocks depending on node history"
    );

    let mut last_number: Option<u64> = None;
    for block in arr {
        let num_str = block["number"]
            .as_str()
            .expect("block.number should be a string");
        let num: u64 = num_str.parse().expect("block.number should parse as u64");

        if let Some(prev) = last_number {
            assert!(
                num >= prev,
                "Blocks should be sorted in ascending order by number"
            );
        }
        last_number = Some(num);
    }

    Ok(())
}

#[tokio::test]
async fn test_blocks_range_invalid_params() -> Result<()> {
    init_tracing();

    let api_url = env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let client = TestClient::new(api_url);

    client.wait_for_ready(API_READY_TIMEOUT_SECONDS).await?;

    let test_cases = vec![
        ("/v1/blocks", "range query parameter must be inputted"),
        ("/v1/blocks?range=", "Incorrect range format"),
        ("/v1/blocks?range=10", "Incorrect range format"),
        ("/v1/blocks?range=10-", "Incorrect range format"),
        ("/v1/blocks?range=-10", "Incorrect range format"),
        (
            "/v1/blocks?range=a-b",
            "Inputted min value for range must be an unsigned integer",
        ),
        (
            "/v1/blocks?range=10-9",
            "Inputted min value cannot be greater than or equal to the max value",
        ),
        (
            "/v1/blocks?range=0-1000",
            "Inputted range is greater than the 500 range limit",
        ),
    ];

    for (endpoint, expected_error) in test_cases {
        let (status, json) = client.get_json(endpoint).await?;
        assert_eq!(
            status.as_u16(),
            400,
            "Endpoint {} should return 400 for invalid range",
            endpoint
        );

        let error_msg = json.get("error").and_then(|e| e.as_str()).unwrap_or("");
        assert!(
            error_msg.contains(expected_error),
            "Endpoint {} - expected error containing '{}', got '{}'",
            endpoint,
            expected_error,
            error_msg
        );
    }

    Ok(())
}

#[tokio::test]
async fn test_blocks_range_use_rc_block_matches_sidecar_fixture() -> Result<()> {
    init_tracing();

    let api_url = env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let client = TestClient::new(api_url);

    client
        .wait_for_ready(API_READY_TIMEOUT_SECONDS)
        .await
        .context("Local API is not ready")?;

    let endpoint = "/v1/blocks?range=10293194-10293197&useRcBlock=true";

    let (status, local_json) = client.get_json(endpoint).await?;
    assert!(status.is_success(), "Local API returned status {}", status);

    let fixture_path =
        get_fixture_path("asset-hub-polkadot/blocks_range_use_rc_block_10293194-10293197.json")?;
    let fixture_content = fs::read_to_string(&fixture_path)
        .with_context(|| format!("Failed to read fixture file: {:?}", fixture_path))?;
    let expected_json: Value = serde_json::from_str(&fixture_content)
        .context("Failed to parse expected response from fixture")?;

    let local_array = local_json
        .as_array()
        .expect("Local response is not an array");
    let expected_array = expected_json
        .as_array()
        .expect("Expected response is not an array");

    assert_eq!(
        local_array.len(),
        expected_array.len(),
        "Block count mismatch: local={}, expected={}",
        local_array.len(),
        expected_array.len()
    );

    let comparison_result = compare_json(&local_json, &expected_json, &[])?;

    if !comparison_result.is_match() {
        let diff_output = comparison_result.format_diff(&expected_json, &local_json);
        println!("{}", diff_output);
    }

    assert!(
        comparison_result.is_match(),
        "Found {} difference(s) between local and expected responses",
        comparison_result.differences().len()
    );

    Ok(())
}

#[tokio::test]
async fn test_blocks_range_use_rc_block_empty_results() -> Result<()> {
    init_tracing();

    let api_url = env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let client = TestClient::new(api_url);

    client
        .wait_for_ready(API_READY_TIMEOUT_SECONDS)
        .await
        .context("Local API is not ready")?;

    let rc_block_number = 10293195u64;
    let endpoint = format!(
        "/v1/blocks?range={}-{}&useRcBlock=true",
        rc_block_number, rc_block_number
    );

    let (status, json) = client.get_json(&endpoint).await?;
    assert!(status.is_success(), "Local API returned status {}", status);

    let arr = json.as_array().expect("Response is not an array");
    assert!(
        arr.is_empty(),
        "Expected empty array for RC block {}, but got {} block(s)",
        rc_block_number,
        arr.len()
    );

    Ok(())
}

fn get_fixture_path(filename: &str) -> Result<PathBuf> {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let fixture_path = PathBuf::from(manifest_dir)
        .join("tests")
        .join("fixtures")
        .join(filename);

    if !fixture_path.exists() {
        anyhow::bail!("Fixture file not found: {:?}", fixture_path);
    }

    Ok(fixture_path)
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
}
