use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;

pub fn compare_json(
    actual: &Value,
    expected: &Value,
    ignore_fields: &[&str],
) -> Result<ComparisonResult> {
    let mut differences = Vec::new();

    compare_json_recursive(actual, expected, ignore_fields, "", &mut differences);

    if differences.is_empty() {
        Ok(ComparisonResult::Match)
    } else {
        Ok(ComparisonResult::Mismatch { differences })
    }
}

fn compare_json_recursive(
    actual: &Value,
    expected: &Value,
    ignore_fields: &[&str],
    path: &str,
    differences: &mut Vec<String>,
) {
    match (actual, expected) {
        (Value::Object(actual_obj), Value::Object(expected_obj)) => {
            for (key, expected_val) in expected_obj.iter() {
                let current_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", path, key)
                };

                // Skip ignored fields
                if ignore_fields
                    .iter()
                    .any(|&field| current_path.ends_with(field) || current_path == field)
                {
                    continue;
                }

                if let Some(actual_val) = actual_obj.get(key) {
                    compare_json_recursive(
                        actual_val,
                        expected_val,
                        ignore_fields,
                        &current_path,
                        differences,
                    );
                } else {
                    differences.push(format!("Missing field: {}", current_path));
                }
            }

            // Check for extra fields in actual (optional - can be made configurable)
            for key in actual_obj.keys() {
                if !expected_obj.contains_key(key) {
                    let current_path = if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{}.{}", path, key)
                    };
                    // Only warn about extra fields, don't fail
                    differences.push(format!("Extra field (not in expected): {}", current_path));
                }
            }
        }
        (Value::Array(actual_arr), Value::Array(expected_arr)) => {
            if actual_arr.len() != expected_arr.len() {
                differences.push(format!(
                    "Array length mismatch at {}: expected {}, got {}",
                    path,
                    expected_arr.len(),
                    actual_arr.len()
                ));
            } else {
                for (i, (actual_val, expected_val)) in
                    actual_arr.iter().zip(expected_arr.iter()).enumerate()
                {
                    let current_path = format!("{}[{}]", path, i);
                    compare_json_recursive(
                        actual_val,
                        expected_val,
                        ignore_fields,
                        &current_path,
                        differences,
                    );
                }
            }
        }
        (actual_val, expected_val) => {
            if actual_val != expected_val {
                differences.push(format!(
                    "Value mismatch at {}: expected {:?}, got {:?}",
                    path, expected_val, actual_val
                ));
            }
        }
    }
}

#[derive(Debug)]
pub enum ComparisonResult {
    Match,
    Mismatch { differences: Vec<String> },
}

impl ComparisonResult {
    pub fn is_match(&self) -> bool {
        matches!(self, ComparisonResult::Match)
    }

    pub fn differences(&self) -> &[String] {
        match self {
            ComparisonResult::Match => &[],
            ComparisonResult::Mismatch { differences } => differences,
        }
    }
}

/// Replace placeholders in a path template
pub fn replace_placeholders(template: &str, replacements: &HashMap<String, String>) -> String {
    let mut result = template.to_string();
    for (key, value) in replacements {
        result = result.replace(&format!("{{{}}}", key), value);
    }
    result
}

/// Build query string from parameters
pub fn build_query_string(params: &std::collections::HashMap<String, String>) -> String {
    if params.is_empty() {
        String::new()
    } else {
        let pairs: Vec<(&String, &String)> = params.iter().collect();
        serde_urlencoded::to_string(pairs)
            .map(|encoded| format!("?{}", encoded))
            .unwrap_or_else(|_| String::new())
    }
}
