use anyhow::Result;
use integration_tests::client::TestClient;
use std::env;

/// Test basic endpoints that are already implemented
#[tokio::test]
async fn test_basic_endpoints() -> Result<()> {
    init_tracing();

    let api_url = env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let client = TestClient::new(api_url);

    // Wait for API to be ready
    client.wait_for_ready(30).await?;

    // Test health endpoint
    tracing::info!("Testing /v1/health endpoint");
    let (status, health_json) = client.get_json("/v1/health").await?;
    assert!(status.is_success(), "Health endpoint should return success status");
    assert_eq!(health_json["status"], "ok", "Health status should be 'ok'");

    // Test version endpoint
    tracing::info!("Testing /v1/version endpoint");
    let (status, version_json) = client.get_json("/v1/version").await?;
    assert!(status.is_success(), "Version endpoint should return success status");
    assert!(
        version_json["version"].is_string(),
        "Version should be a string"
    );
    assert!(
        !version_json["version"].as_str().unwrap().is_empty(),
        "Version should not be empty"
    );

    println!("✓ Basic endpoints test passed");
    Ok(())
}

#[tokio::test]
async fn test_health_consistency() -> Result<()> {
    init_tracing();

    let api_url = env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let client = TestClient::new(api_url);

    client.wait_for_ready(30).await?;

    // Test health endpoint multiple times to ensure consistency
    for i in 0..5 {
        let (status, health_json) = client.get_json("/v1/health").await?;
        assert!(
            status.is_success(),
            "Health endpoint should return success status (attempt {})",
            i + 1
        );
        assert_eq!(
            health_json["status"],
            "ok",
            "Health status should be 'ok' (attempt {})",
            i + 1
        );
    }

    println!("✓ Health endpoint consistency test passed");
    Ok(())
}

/// Test version endpoint response structure
#[tokio::test]
async fn test_version_structure() -> Result<()> {
    init_tracing();

    let api_url = env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let client = TestClient::new(api_url);

    client.wait_for_ready(30).await?;

    let (status, version_json) = client.get_json("/v1/version").await?;
    assert!(status.is_success(), "Version endpoint should return success status");

    // Validate version structure
    assert!(version_json.is_object(), "Version response should be an object");
    assert!(
        version_json.get("version").is_some(),
        "Version response should contain 'version' field"
    );
    assert!(
        version_json["version"].is_string(),
        "Version field should be a string"
    );

    let version_str = version_json["version"].as_str().unwrap();
    assert!(!version_str.is_empty(), "Version string should not be empty");

    // Version should be in semver format (e.g., "0.1.0")
    let parts: Vec<&str> = version_str.split('.').collect();
    assert!(
        parts.len() >= 2,
        "Version should be in semver format (e.g., 0.1.0)"
    );

    println!("✓ Version structure test passed (version: {})", version_str);
    Ok(())
}

/// Test invalid endpoints return 404
#[tokio::test]
async fn test_invalid_endpoints() -> Result<()> {
    init_tracing();

    let api_url = env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let client = TestClient::new(api_url);

    client.wait_for_ready(30).await?;

    let invalid_endpoints = vec![
        "/v1/invalid-endpoint",
        "/v1/blocks/invalid-block-id",
        "/v1/accounts/invalid-account-id",
        "/v1/runtime/invalid-metadata",
        "/invalid-path",
    ];

    for endpoint in invalid_endpoints {
        let response = client.get(endpoint).await?;
        assert_eq!(
            response.status.as_u16(),
            404,
            "Invalid endpoint {} should return 404",
            endpoint
        );
    }

    println!("✓ Invalid endpoints test passed");
    Ok(())
}

/// Test concurrent requests
#[tokio::test]
async fn test_concurrent_requests() -> Result<()> {
    init_tracing();

    let api_url = env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let client = TestClient::new(api_url);

    client.wait_for_ready(30).await?;

    // Make 10 concurrent requests
    let mut handles = Vec::new();
    for i in 0..10 {
        let client = client.clone();
        let handle = tokio::spawn(async move {
            let (status, health_json) = client.get_json("/v1/health").await?;
            anyhow::ensure!(
                status.is_success(),
                "Request {} should succeed",
                i
            );
            anyhow::ensure!(
                health_json["status"] == "ok",
                "Request {} should return ok status",
                i
            );
            Ok::<(), anyhow::Error>(())
        });
        handles.push(handle);
    }

    // Wait for all requests to complete
    for (i, handle) in handles.into_iter().enumerate() {
        handle.await??;
        tracing::debug!("Concurrent request {} completed", i);
    }

    println!("✓ Concurrent requests test passed");
    Ok(())
}

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
}
