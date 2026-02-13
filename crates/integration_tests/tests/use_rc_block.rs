use anyhow::{Context, Result};
use colored::Colorize;
use integration_tests::{client::TestClient, constants::API_READY_TIMEOUT_SECONDS};
use serde_json::Value;
use std::env;
use std::fs;
use std::path::PathBuf;

/// Helper to assert that a useRcBlock endpoint returns an empty array for
/// a Relay Chain block that contains no Asset Hub blocks.
async fn assert_use_rc_block_empty(client: &TestClient, endpoint: &str) -> Result<()> {
    let (status, json) = client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .with_context(|| format!("Failed to fetch {}", endpoint))?;

    assert!(
        status.is_success(),
        "Endpoint {} returned status {}",
        endpoint,
        status
    );

    let array = json.as_array().with_context(|| {
        format!(
            "Expected array for {}, got: {}",
            endpoint,
            serde_json::to_string_pretty(&json).unwrap_or_default()
        )
    })?;

    assert!(
        array.is_empty(),
        "Expected empty array for {}, got {} element(s)",
        endpoint,
        array.len()
    );

    println!("{} {} -> []", "✓".green(), endpoint);
    Ok(())
}

#[tokio::test]
async fn test_use_rc_block_pallets_empty_response() -> Result<()> {
    init_tracing();

    let api_url = env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let local_client = TestClient::new(api_url);

    local_client
        .wait_for_ready(API_READY_TIMEOUT_SECONDS)
        .await
        .context("Local API is not ready")?;

    // RC block 25000001 does not include any Asset Hub blocks
    let rc_block = 25000001;

    println!(
        "\n{} Testing pallet endpoints return empty array for RC block {} (no AH blocks)",
        "Testing".cyan().bold(),
        rc_block.to_string().yellow()
    );
    println!("{}", "═".repeat(80).bright_white());

    let endpoints = vec![
        format!("/pallets/balances/dispatchables?useRcBlock=true&at={}", rc_block),
        format!("/pallets/balances/dispatchables/transferAllowDeath?useRcBlock=true&at={}", rc_block),
        format!("/pallets/balances/storage?useRcBlock=true&at={}", rc_block),
        format!("/pallets/balances/storage/totalIssuance?useRcBlock=true&at={}", rc_block),
        format!("/pallets/balances/consts?useRcBlock=true&at={}", rc_block),
        format!("/pallets/balances/consts/existentialDeposit?useRcBlock=true&at={}", rc_block),
        format!("/pallets/balances/errors?useRcBlock=true&at={}", rc_block),
        format!("/pallets/balances/errors/InsufficientBalance?useRcBlock=true&at={}", rc_block),
        format!("/pallets/balances/events?useRcBlock=true&at={}", rc_block),
        format!("/pallets/balances/events/Transfer?useRcBlock=true&at={}", rc_block),
    ];

    for endpoint in &endpoints {
        assert_use_rc_block_empty(&local_client, endpoint).await?;
    }

    println!("{}", "═".repeat(80).bright_white());
    println!(
        "{} All {} pallet endpoints returned empty array as expected",
        "✓".green().bold(),
        endpoints.len()
    );
    Ok(())
}

#[tokio::test]
async fn test_use_rc_block_empty_response() -> Result<()> {
    init_tracing();

    let api_url = env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let local_client = TestClient::new(api_url);

    local_client
        .wait_for_ready(API_READY_TIMEOUT_SECONDS)
        .await
        .context("Local API is not ready")?;

    // Block 10554958 is a Relay Chain block that doesn't include any Asset Hub blocks
    let rc_block_number = 10554958;
    let endpoint = format!("/blocks/{}?useRcBlock=true", rc_block_number);

    println!(
        "\n{} Testing useRcBlock returns empty array for RC block {} (no AH blocks)",
        "Testing".cyan().bold(),
        rc_block_number.to_string().yellow()
    );
    println!("{}", "═".repeat(80).bright_white());

    println!(
        "{} Fetching from local API: {}{}",
        "→".cyan(),
        local_client.base_url(),
        endpoint
    );
    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    println!("{} Local API response: {}", "✓".green(), "OK".green());

    let local_array = local_json
        .as_array()
        .expect("Local response is not an array");

    assert!(
        local_array.is_empty(),
        "Expected empty array for RC block {}, but got {} block(s)",
        rc_block_number,
        local_array.len()
    );

    println!("{} Response is empty array as expected", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_use_rc_block_head_header() -> Result<()> {
    init_tracing();

    let api_url = env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let local_client = TestClient::new(api_url);

    local_client
        .wait_for_ready(API_READY_TIMEOUT_SECONDS)
        .await
        .context("Local API is not ready")?;

    let endpoint = "/blocks/head/header?useRcBlock=true";

    println!(
        "\n{} Testing useRcBlock for /blocks/head/header",
        "Testing".cyan().bold()
    );
    println!("{}", "═".repeat(80).bright_white());

    println!(
        "{} Fetching from local API: {}{}",
        "→".cyan(),
        local_client.base_url(),
        endpoint
    );
    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    println!("{} Local API response: {}", "✓".green(), "OK".green());

    println!("{} Loading expected structure from fixture", "→".cyan());
    let fixture_path = get_fixture_path("use_rc_block_head_header_structure.json")?;
    let fixture_content = fs::read_to_string(&fixture_path)
        .with_context(|| format!("Failed to read fixture file: {:?}", fixture_path))?;
    let expected_structure: Value = serde_json::from_str(&fixture_content)
        .context("Failed to parse expected structure from fixture")?;
    println!("{} Expected structure loaded from fixture", "✓".green());

    println!(
        "\n{} Validating response structure and types...",
        "→".cyan().bold()
    );

    let local_array = local_json
        .as_array()
        .expect("Local response is not an array");
    let expected_array = expected_structure
        .as_array()
        .expect("Expected structure is not an array");

    println!(
        "  {} Local response contains {} block(s)",
        "✓".green(),
        local_array.len()
    );

    assert!(!local_array.is_empty(), "Local response is empty");

    let local_block = &local_array[0];
    let expected_block = &expected_array[0];

    validate_block_structure(local_block, expected_block);

    println!("\n{} Structure validation passed!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

fn validate_block_structure(local: &Value, expected: &Value) {
    let expected_obj = expected
        .as_object()
        .expect("Expected structure is not an object");

    let local_obj = local.as_object().expect("Local response is not an object");

    let mut errors = Vec::new();

    for (field, expected_value) in expected_obj.iter() {
        match local_obj.get(field) {
            Some(local_value) => {
                if std::mem::discriminant(local_value) != std::mem::discriminant(expected_value) {
                    errors.push(format!(
                        "  {} {}: type mismatch - local={}, expected={}",
                        "✗".red(),
                        field,
                        value_type_name(local_value),
                        value_type_name(expected_value)
                    ));
                } else {
                    println!(
                        "  {} {}: {} (type matches)",
                        "✓".green(),
                        field,
                        value_type_name(local_value)
                    );
                }
            }
            None => {
                errors.push(format!(
                    "  {} {}: missing in local but present in expected structure",
                    "✗".red(),
                    field
                ));
            }
        }
    }

    for field in local_obj.keys() {
        if !expected_obj.contains_key(field) {
            errors.push(format!(
                "  {} {}: present in local but not in expected structure",
                "✗".red(),
                field
            ));
        }
    }

    if !errors.is_empty() {
        println!("\n{} Structure validation errors:", "✗".red().bold());
        for error in &errors {
            println!("{}", error);
        }
    }

    assert!(
        errors.is_empty(),
        "Found {} structure error(s)",
        errors.len()
    );
}

fn value_type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
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

#[tokio::test]
async fn test_use_rc_block_head() -> Result<()> {
    init_tracing();

    let api_url = env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let local_client = TestClient::new(api_url);

    local_client
        .wait_for_ready(API_READY_TIMEOUT_SECONDS)
        .await
        .context("Local API is not ready")?;

    let endpoint = "/blocks/head?useRcBlock=true";

    println!(
        "\n{} Testing useRcBlock for /blocks/head",
        "Testing".cyan().bold()
    );
    println!("{}", "═".repeat(80).bright_white());

    println!(
        "{} Fetching from local API: {}{}",
        "→".cyan(),
        local_client.base_url(),
        endpoint
    );
    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    println!("{} Local API response: {}", "✓".green(), "OK".green());

    println!(
        "\n{} Validating response structure and types...",
        "→".cyan().bold()
    );

    let local_array = local_json
        .as_array()
        .expect("Local response is not an array");

    println!(
        "  {} Local response contains {} block(s)",
        "✓".green(),
        local_array.len()
    );

    assert!(!local_array.is_empty(), "Local response is empty");

    let local_block = &local_array[0];
    validate_block_head_structure(local_block)?;

    validate_use_rc_block_fields(local_block)?;

    println!("\n{} Structure validation passed!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

#[tokio::test]
async fn test_use_rc_block_head_finalized_false() -> Result<()> {
    init_tracing();

    let api_url = env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let local_client = TestClient::new(api_url);

    local_client
        .wait_for_ready(API_READY_TIMEOUT_SECONDS)
        .await
        .context("Local API is not ready")?;

    let endpoint = "/blocks/head?useRcBlock=true&finalized=false";

    println!(
        "\n{} Testing useRcBlock for /blocks/head with finalized=false",
        "Testing".cyan().bold()
    );
    println!("{}", "═".repeat(80).bright_white());

    println!(
        "{} Fetching from local API: {}{}",
        "→".cyan(),
        local_client.base_url(),
        endpoint
    );
    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    println!("{} Local API response: {}", "✓".green(), "OK".green());

    let local_array = local_json
        .as_array()
        .expect("Local response is not an array");

    println!(
        "  {} Local response contains {} block(s)",
        "✓".green(),
        local_array.len()
    );

    assert!(!local_array.is_empty(), "Local response is empty");

    println!("\n{} Canonical head test passed!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

fn validate_block_head_structure(block: &Value) -> Result<()> {
    let block_obj = block.as_object().context("Block is not an object")?;

    let required_fields = vec![
        ("number", "string"),
        ("hash", "string"),
        ("parentHash", "string"),
        ("stateRoot", "string"),
        ("extrinsicsRoot", "string"),
        ("logs", "array"),
        ("onInitialize", "object"),
        ("extrinsics", "array"),
        ("onFinalize", "object"),
    ];

    let mut errors = Vec::new();

    for (field, expected_type) in required_fields {
        match block_obj.get(field) {
            Some(value) => {
                let actual_type = value_type_name(value);
                if actual_type != expected_type {
                    errors.push(format!(
                        "  {} {}: type mismatch - expected {}, got {}",
                        "✗".red(),
                        field,
                        expected_type,
                        actual_type
                    ));
                } else {
                    println!("  {} {}: {} ✓", "✓".green(), field, actual_type);
                }
            }
            None => {
                errors.push(format!("  {} {}: missing required field", "✗".red(), field));
            }
        }
    }

    if !errors.is_empty() {
        println!("\n{} Structure validation errors:", "✗".red().bold());
        for error in &errors {
            println!("{}", error);
        }
        anyhow::bail!("Found {} structure error(s)", errors.len());
    }

    Ok(())
}

fn validate_use_rc_block_fields(block: &Value) -> Result<()> {
    let block_obj = block.as_object().context("Block is not an object")?;

    let rc_fields = vec![
        ("rcBlockHash", "string"),
        ("rcBlockNumber", "string"),
        ("ahTimestamp", "string"),
    ];

    let mut errors = Vec::new();

    println!("\n{} Validating useRcBlock-specific fields...", "→".cyan());

    for (field, expected_type) in rc_fields {
        match block_obj.get(field) {
            Some(value) => {
                let actual_type = value_type_name(value);
                if actual_type != expected_type {
                    errors.push(format!(
                        "  {} {}: type mismatch - expected {}, got {}",
                        "✗".red(),
                        field,
                        expected_type,
                        actual_type
                    ));
                } else {
                    println!("  {} {}: {} ✓", "✓".green(), field, actual_type);
                }
            }
            None => {
                errors.push(format!(
                    "  {} {}: missing required useRcBlock field",
                    "✗".red(),
                    field
                ));
            }
        }
    }

    if !errors.is_empty() {
        println!("\n{} useRcBlock field validation errors:", "✗".red().bold());
        for error in &errors {
            println!("{}", error);
        }
        anyhow::bail!("Found {} useRcBlock field error(s)", errors.len());
    }

    Ok(())
}

// ================================================================================================
// Tests for /blocks/{blockId}/extrinsics-raw?useRcBlock=true
// ================================================================================================

#[tokio::test]
async fn test_use_rc_block_extrinsics_raw_structure() -> Result<()> {
    init_tracing();

    let api_url = env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let local_client = TestClient::new(api_url);

    local_client
        .wait_for_ready(API_READY_TIMEOUT_SECONDS)
        .await
        .context("Local API is not ready")?;

    let rc_block_number = 10554957;
    let endpoint = format!("/blocks/{}/extrinsics-raw?useRcBlock=true", rc_block_number);

    println!(
        "\n{} Testing useRcBlock for /blocks/{}/extrinsics-raw - Structure validation",
        "Testing".cyan().bold(),
        rc_block_number
    );
    println!("{}", "═".repeat(80).bright_white());

    println!(
        "{} Fetching from local API: {}{}",
        "→".cyan(),
        local_client.base_url(),
        endpoint
    );
    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    println!("{} Local API response: {}", "✓".green(), "OK".green());

    println!("\n{} Validating response structure...", "→".cyan().bold());

    let local_array = local_json
        .as_array()
        .expect("Local response is not an array");

    println!(
        "  {} Local response contains {} block(s)",
        "✓".green(),
        local_array.len()
    );

    assert!(!local_array.is_empty(), "Local response is empty");

    // Validate first block structure
    let local_block = &local_array[0];
    validate_extrinsics_raw_structure(local_block)?;
    validate_use_rc_block_fields(local_block)?;

    println!("\n{} Structure validation passed!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

fn validate_extrinsics_raw_structure(block: &Value) -> Result<()> {
    let block_obj = block.as_object().context("Block is not an object")?;

    let required_fields = vec![
        ("parentHash", "string"),
        ("number", "string"),
        ("stateRoot", "string"),
        ("extrinsicRoot", "string"),
        ("digest", "object"),
        ("extrinsics", "array"),
    ];

    let mut errors = Vec::new();

    for (field, expected_type) in required_fields {
        match block_obj.get(field) {
            Some(value) => {
                let actual_type = value_type_name(value);
                if actual_type != expected_type {
                    errors.push(format!(
                        "  {} {}: type mismatch - expected {}, got {}",
                        "✗".red(),
                        field,
                        expected_type,
                        actual_type
                    ));
                } else {
                    println!("  {} {}: {} ✓", "✓".green(), field, actual_type);
                }
            }
            None => {
                errors.push(format!("  {} {}: missing required field", "✗".red(), field));
            }
        }
    }

    // Validate digest structure
    if let Some(digest) = block_obj.get("digest")
        && let Some(digest_obj) = digest.as_object()
    {
        if let Some(logs) = digest_obj.get("logs") {
            if logs.as_array().is_some() {
                println!("  {} digest.logs: array ✓", "✓".green());
            } else {
                errors.push(format!(
                    "  {} digest.logs: expected array, got {}",
                    "✗".red(),
                    value_type_name(logs)
                ));
            }
        } else {
            errors.push(format!("  {} digest.logs: missing", "✗".red()));
        }
    }

    if !errors.is_empty() {
        println!("\n{} Structure validation errors:", "✗".red().bold());
        for error in &errors {
            println!("{}", error);
        }
        anyhow::bail!("Found {} structure error(s)", errors.len());
    }

    Ok(())
}

// ================================================================================================
// Tests for /blocks/{blockId}/extrinsics/{extrinsicIndex}?useRcBlock=true
// ================================================================================================

#[tokio::test]
async fn test_use_rc_block_extrinsic_structure() -> Result<()> {
    init_tracing();

    let api_url = env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let local_client = TestClient::new(api_url);

    local_client
        .wait_for_ready(API_READY_TIMEOUT_SECONDS)
        .await
        .context("Local API is not ready")?;

    let rc_block_number = 10554957;
    let extrinsic_index = 0;
    let endpoint = format!(
        "/blocks/{}/extrinsics/{}?useRcBlock=true",
        rc_block_number, extrinsic_index
    );

    println!(
        "\n{} Testing useRcBlock for /blocks/{}/extrinsics/{} - Structure validation",
        "Testing".cyan().bold(),
        rc_block_number,
        extrinsic_index
    );
    println!("{}", "═".repeat(80).bright_white());

    println!(
        "{} Fetching from local API: {}{}",
        "→".cyan(),
        local_client.base_url(),
        endpoint
    );
    let (local_status, local_json) = local_client
        .get_json(&format!("/v1{}", endpoint))
        .await
        .context("Failed to fetch from local API")?;

    assert!(
        local_status.is_success(),
        "Local API returned status {}",
        local_status
    );

    println!("{} Local API response: {}", "✓".green(), "OK".green());

    println!("\n{} Validating response structure...", "→".cyan().bold());

    let local_array = local_json
        .as_array()
        .expect("Local response is not an array");

    println!(
        "  {} Local response contains {} item(s)",
        "✓".green(),
        local_array.len()
    );

    assert!(!local_array.is_empty(), "Local response is empty");

    // Validate first extrinsic structure
    let local_extrinsic = &local_array[0];
    validate_extrinsic_structure(local_extrinsic)?;
    validate_use_rc_block_fields(local_extrinsic)?;

    println!("\n{} Structure validation passed!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
    Ok(())
}

fn validate_extrinsic_structure(extrinsic_response: &Value) -> Result<()> {
    let response_obj = extrinsic_response
        .as_object()
        .context("Extrinsic response is not an object")?;

    // Validate top-level structure
    let top_level_fields = vec![("at", "object"), ("extrinsics", "object")];

    let mut errors = Vec::new();

    for (field, expected_type) in top_level_fields {
        match response_obj.get(field) {
            Some(value) => {
                let actual_type = value_type_name(value);
                if actual_type != expected_type {
                    errors.push(format!(
                        "  {} {}: type mismatch - expected {}, got {}",
                        "✗".red(),
                        field,
                        expected_type,
                        actual_type
                    ));
                } else {
                    println!("  {} {}: {} ✓", "✓".green(), field, actual_type);
                }
            }
            None => {
                errors.push(format!("  {} {}: missing required field", "✗".red(), field));
            }
        }
    }

    // Validate 'at' structure
    if let Some(at) = response_obj.get("at")
        && let Some(at_obj) = at.as_object()
    {
        for field in &["height", "hash"] {
            match at_obj.get(*field) {
                Some(value) if value.is_string() => {
                    println!("  {} at.{}: string ✓", "✓".green(), field);
                }
                Some(value) => {
                    errors.push(format!(
                        "  {} at.{}: expected string, got {}",
                        "✗".red(),
                        field,
                        value_type_name(value)
                    ));
                }
                None => {
                    errors.push(format!("  {} at.{}: missing", "✗".red(), field));
                }
            }
        }
    }

    // Validate 'extrinsics' structure
    if let Some(extrinsics) = response_obj.get("extrinsics")
        && let Some(ext_obj) = extrinsics.as_object()
    {
        let extrinsic_fields = vec![
            ("method", "object"),
            ("signature", "null"),
            ("nonce", "null"),
            ("args", "object"),
            ("tip", "null"),
            ("hash", "string"),
            ("info", "object"),
            ("era", "object"),
            ("events", "array"),
            ("success", "boolean"),
            ("paysFee", "boolean"),
        ];

        for (field, _expected_type) in extrinsic_fields {
            if ext_obj.contains_key(field) {
                println!("  {} extrinsics.{}: present ✓", "✓".green(), field);
            } else {
                errors.push(format!("  {} extrinsics.{}: missing", "✗".red(), field));
            }
        }
    }

    if !errors.is_empty() {
        println!("\n{} Structure validation errors:", "✗".red().bold());
        for error in &errors {
            println!("{}", error);
        }
        anyhow::bail!("Found {} structure error(s)", errors.len());
    }

    Ok(())
}
