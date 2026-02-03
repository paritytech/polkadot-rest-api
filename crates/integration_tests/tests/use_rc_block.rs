use anyhow::{Context, Result};
use colored::Colorize;
use integration_tests::{
    client::TestClient, constants::API_READY_TIMEOUT_SECONDS, utils::compare_json,
};
use serde_json::Value;
use std::env;
use std::fs;
use std::path::PathBuf;

#[tokio::test]
async fn test_use_rc_block_comparison() -> Result<()> {
    init_tracing();

    let api_url = env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let local_client = TestClient::new(api_url);

    local_client
        .wait_for_ready(API_READY_TIMEOUT_SECONDS)
        .await
        .context("Local API is not ready")?;

    let rc_block_number = 10554957;
    let endpoint = format!("/blocks/{}?useRcBlock=true", rc_block_number);

    println!(
        "\n{} Comparing useRcBlock responses for RC block {}",
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

    println!(
        "{} Loading expected response from sidecar fixture",
        "→".cyan()
    );
    let fixture_path = get_fixture_path("use_rc_block_10554957.json")?;
    let fixture_content = fs::read_to_string(&fixture_path)
        .with_context(|| format!("Failed to read fixture file: {:?}", fixture_path))?;
    let sidecar_json: Value = serde_json::from_str(&fixture_content)
        .context("Failed to parse expected sidecar response from fixture")?;
    println!("{} Expected response loaded from fixture", "✓".green());

    println!("\n{} Validating responses...", "→".cyan().bold());

    let local_array = local_json
        .as_array()
        .expect("Local response is not an array");
    let sidecar_array = sidecar_json
        .as_array()
        .expect("Sidecar response is not an array");

    println!(
        "  {} Local response contains {} block(s)",
        "✓".green(),
        local_array.len()
    );
    println!(
        "  {} Sidecar response contains {} block(s)",
        "✓".green(),
        sidecar_array.len()
    );

    assert_eq!(
        local_array.len(),
        sidecar_array.len(),
        "Block count mismatch: local={}, sidecar={}",
        local_array.len(),
        sidecar_array.len()
    );

    println!(
        "  {} Block counts match: {}",
        "✓".green(),
        local_array.len()
    );

    println!("\n{} Comparing JSON responses...", "→".cyan().bold());
    let comparison_result = compare_json(&local_json, &sidecar_json, &[])?;

    if !comparison_result.is_match() {
        println!("{} JSON responses differ:", "✗".red().bold());
        let diff_output = comparison_result.format_diff(&sidecar_json, &local_json);
        println!("{}", diff_output);
        println!("{}", "═".repeat(80).bright_white());
    }

    assert!(
        comparison_result.is_match(),
        "Found {} difference(s) between local and expected responses",
        comparison_result.differences().len()
    );

    println!("{} All JSON responses match!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
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

#[tokio::test]
async fn test_use_rc_block_header_by_id() -> Result<()> {
    init_tracing();

    let api_url = env::var("API_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());
    let local_client = TestClient::new(api_url);

    local_client
        .wait_for_ready(API_READY_TIMEOUT_SECONDS)
        .await
        .context("Local API is not ready")?;

    let rc_block_number = 10293194;
    let endpoint = format!("/blocks/{}/header?useRcBlock=true", rc_block_number);

    println!(
        "\n{} Testing useRcBlock for /blocks/{}/header",
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

    println!("{} Loading expected response from fixture", "→".cyan());
    let fixture_path = get_fixture_path("use_rc_block_header_10293194.json")?;
    let fixture_content = fs::read_to_string(&fixture_path)
        .with_context(|| format!("Failed to read fixture file: {:?}", fixture_path))?;
    let expected_json: Value = serde_json::from_str(&fixture_content)
        .context("Failed to parse expected response from fixture")?;
    println!("{} Expected response loaded from fixture", "✓".green());

    println!("\n{} Validating responses...", "→".cyan().bold());

    let local_array = local_json
        .as_array()
        .expect("Local response is not an array");
    let expected_array = expected_json
        .as_array()
        .expect("Expected response is not an array");

    println!(
        "  {} Local response contains {} block(s)",
        "✓".green(),
        local_array.len()
    );
    println!(
        "  {} Expected response contains {} block(s)",
        "✓".green(),
        expected_array.len()
    );

    assert_eq!(
        local_array.len(),
        expected_array.len(),
        "Block count mismatch: local={}, expected={}",
        local_array.len(),
        expected_array.len()
    );

    println!("\n{} Comparing JSON responses...", "→".cyan().bold());
    let comparison_result = compare_json(&local_json, &expected_json, &[])?;

    if !comparison_result.is_match() {
        println!("{} JSON responses differ:", "✗".red().bold());
        let diff_output = comparison_result.format_diff(&expected_json, &local_json);
        println!("{}", diff_output);
        println!("{}", "═".repeat(80).bright_white());
    }

    assert!(
        comparison_result.is_match(),
        "Found {} difference(s) between local and expected responses",
        comparison_result.differences().len()
    );

    println!("{} All JSON responses match!", "✓".green().bold());
    println!("{}", "═".repeat(80).bright_white());
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
