use anyhow::Result;
use colored::Colorize;
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

#[derive(Debug, Clone)]
pub enum Difference {
    ValueMismatch {
        path: String,
        expected: Value,
        actual: Value,
    },
    MissingField {
        path: String,
    },
    ExtraField {
        path: String,
    },
    ArrayLengthMismatch {
        path: String,
        expected_len: usize,
        actual_len: usize,
    },
}

fn compare_json_recursive(
    actual: &Value,
    expected: &Value,
    ignore_fields: &[&str],
    path: &str,
    differences: &mut Vec<Difference>,
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
                    differences.push(Difference::MissingField {
                        path: current_path,
                    });
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
                    differences.push(Difference::ExtraField {
                        path: current_path,
                    });
                }
            }
        }
        (Value::Array(actual_arr), Value::Array(expected_arr)) => {
            if actual_arr.len() != expected_arr.len() {
                differences.push(Difference::ArrayLengthMismatch {
                    path: path.to_string(),
                    expected_len: expected_arr.len(),
                    actual_len: actual_arr.len(),
                });

                // Report extra elements in actual
                if actual_arr.len() > expected_arr.len() {
                    for i in expected_arr.len()..actual_arr.len() {
                        let current_path = format!("{}[{}]", path, i);
                        differences.push(Difference::ExtraField {
                            path: current_path,
                        });
                    }
                }

                // Report missing elements (in expected but not in actual)
                if expected_arr.len() > actual_arr.len() {
                    for i in actual_arr.len()..expected_arr.len() {
                        let current_path = format!("{}[{}]", path, i);
                        differences.push(Difference::MissingField {
                            path: current_path,
                        });
                    }
                }
            }

            // Compare overlapping elements even if lengths differ
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
        (actual_val, expected_val) => {
            if actual_val != expected_val {
                differences.push(Difference::ValueMismatch {
                    path: path.to_string(),
                    expected: expected_val.clone(),
                    actual: actual_val.clone(),
                });
            }
        }
    }
}

#[derive(Debug)]
pub enum ComparisonResult {
    Match,
    Mismatch { differences: Vec<Difference> },
}

impl ComparisonResult {
    pub fn is_match(&self) -> bool {
        matches!(self, ComparisonResult::Match)
    }

    pub fn differences(&self) -> &[Difference] {
        match self {
            ComparisonResult::Match => &[],
            ComparisonResult::Mismatch { differences } => differences,
        }
    }

    /// Recursively sort JSON object keys to ensure consistent ordering
    fn sort_json_keys(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut sorted_map = serde_json::Map::new();
                let mut keys: Vec<_> = map.keys().collect();
                keys.sort();

                for key in keys {
                    if let Some(val) = map.get(key) {
                        sorted_map.insert(key.clone(), Self::sort_json_keys(val));
                    }
                }
                Value::Object(sorted_map)
            }
            Value::Array(arr) => {
                Value::Array(arr.iter().map(Self::sort_json_keys).collect())
            }
            other => other.clone(),
        }
    }

    /// Format differences with colored output showing expected vs actual
    ///
    /// Shows a unified JSON view with inline diffs at exact locations
    pub fn format_diff(&self, expected: &Value, actual: &Value) -> String {
        match self {
            ComparisonResult::Match => String::new(),
            ComparisonResult::Mismatch { differences } => {
                let mut output = Vec::new();

                output.push(format!("\n{}", "=".repeat(80).bright_white()));
                output.push(format!("{}", "RESPONSE MISMATCH".bright_yellow().bold()));
                output.push(format!("{}\n", "=".repeat(80).bright_white()));

                // Sort keys to ensure consistent ordering
                let sorted_expected = Self::sort_json_keys(expected);
                let sorted_actual = Self::sort_json_keys(actual);

                // Create a map of paths to differences for quick lookup
                let diff_map = Self::create_diff_map(differences);

                // Generate the unified JSON with inline diffs
                let unified = Self::format_unified_json(&sorted_expected, &sorted_actual, "", &diff_map, 0);
                output.push(unified);

                output.push(String::new());
                output.push(format!("{}", "=".repeat(80).bright_white()));
                output.push(format!(
                    "{} {}",
                    "Total differences:".bright_cyan().bold(),
                    differences.len().to_string().bright_white()
                ));

                output.join("\n")
            }
        }
    }

    /// Create a map of paths to differences for quick lookup
    fn create_diff_map(differences: &[Difference]) -> std::collections::HashMap<String, &Difference> {
        let mut map = std::collections::HashMap::new();
        for diff in differences {
            let path = match diff {
                Difference::ValueMismatch { path, .. } => path,
                Difference::MissingField { path } => path,
                Difference::ExtraField { path } => path,
                Difference::ArrayLengthMismatch { path, .. } => path,
            };
            map.insert(path.clone(), diff);
        }
        map
    }

    /// Format a unified JSON view with inline diffs
    fn format_unified_json(
        expected: &Value,
        actual: &Value,
        current_path: &str,
        _diff_map: &std::collections::HashMap<String, &Difference>,
        indent: usize,
    ) -> String {
        let indent_str = "  ".repeat(indent);
        let mut output = Vec::new();

        match (expected, actual) {
            (Value::Object(exp_obj), Value::Object(act_obj)) => {
                output.push("{".to_string());

                let mut keys: Vec<_> = exp_obj.keys().chain(act_obj.keys()).collect();
                keys.sort();
                keys.dedup();

                for (i, key) in keys.iter().enumerate() {
                    let field_path = if current_path.is_empty() {
                        key.to_string()
                    } else {
                        format!("{}.{}", current_path, key)
                    };

                    let exp_val = exp_obj.get(*key);
                    let act_val = act_obj.get(*key);
                    let is_last = i == keys.len() - 1;
                    let comma = if is_last { "" } else { "," };

                    match (exp_val, act_val) {
                        (Some(e), Some(a)) => {
                            // Both present
                            if e == a {
                                // Same value
                                match e {
                                    Value::Object(_) | Value::Array(_) => {
                                        let val_str = Self::format_value_with_indent(e, indent + 1);
                                        let lines: Vec<&str> = val_str.lines().collect();
                                        // First line: key and opening brace/bracket
                                        output.push(format!("{}  \"{}\": {}",
                                            "  ".repeat(indent), key, lines[0]));
                                        // Subsequent lines already have proper indentation from format_value_with_indent
                                        for (idx, line) in lines.iter().skip(1).enumerate() {
                                            let is_last_line = idx == lines.len() - 2;
                                            if is_last_line {
                                                output.push(format!("{}{}", line, comma));
                                            } else {
                                                output.push(line.to_string());
                                            }
                                        }
                                    }
                                    _ => {
                                        let val_str = Self::value_to_inline_string(e);
                                        output.push(format!("{}  \"{}\": {}{}",
                                            "  ".repeat(indent), key, val_str, comma));
                                    }
                                }
                            } else {
                                // Different - check if it's a complex type or leaf
                                match (e, a) {
                                    (Value::Object(_), Value::Object(_)) | (Value::Array(_), Value::Array(_)) => {
                                        // Recurse for complex types
                                        output.push(format!("{}  \"{}\": {}",
                                            "  ".repeat(indent), key,
                                            Self::format_unified_json(e, a, &field_path, _diff_map, indent + 1)
                                        ));
                                        // Add comma on a separate line if needed
                                        if !comma.is_empty() {
                                            let last_line = output.last_mut().unwrap();
                                            last_line.push_str(comma);
                                        }
                                    }
                                    _ => {
                                        // Leaf value difference - show inline
                                        let actual_str = Self::value_to_inline_string(a);
                                        let expected_str = Self::value_to_inline_string(e);

                                        output.push(format!("- {}\"{}\": {}{}",
                                            "  ".repeat(indent), key, actual_str, comma).red().to_string());
                                        output.push(format!("+ {}\"{}\": {}{}",
                                            "  ".repeat(indent), key, expected_str, comma).green().to_string());
                                    }
                                }
                            }
                        }
                        (Some(e), None) => {
                            // Missing in actual
                            match e {
                                Value::Object(_) | Value::Array(_) => {
                                    let val_str = Self::format_value_with_indent(e, indent + 1);
                                    let lines: Vec<&str> = val_str.lines().collect();
                                    // First line: key and opening brace/bracket
                                    output.push(format!("+ {}\"{}\": {}", "  ".repeat(indent), key, lines[0]).green().to_string());
                                    // Subsequent lines: prefix at leftmost, line already has indentation
                                    for (idx, line) in lines.iter().skip(1).enumerate() {
                                        let is_last_line = idx == lines.len() - 2;
                                        if is_last_line {
                                            output.push(format!("+ {}{}", line, comma).green().to_string());
                                        } else {
                                            output.push(format!("+ {}", line).green().to_string());
                                        }
                                    }
                                }
                                _ => {
                                    let val_str = Self::value_to_inline_string(e);
                                    output.push(format!("+ {}\"{}\": {}{}",
                                        "  ".repeat(indent), key, val_str, comma).green().to_string());
                                }
                            }
                        }
                        (None, Some(a)) => {
                            // Extra in actual
                            match a {
                                Value::Object(_) | Value::Array(_) => {
                                    let val_str = Self::format_value_with_indent(a, indent + 1);
                                    let lines: Vec<&str> = val_str.lines().collect();
                                    // First line: key and opening brace/bracket
                                    output.push(format!("- {}\"{}\": {}", "  ".repeat(indent), key, lines[0]).red().to_string());
                                    // Subsequent lines: prefix at leftmost, line already has indentation
                                    for (idx, line) in lines.iter().skip(1).enumerate() {
                                        let is_last_line = idx == lines.len() - 2;
                                        if is_last_line {
                                            output.push(format!("- {}{}", line, comma).red().to_string());
                                        } else {
                                            output.push(format!("- {}", line).red().to_string());
                                        }
                                    }
                                }
                                _ => {
                                    let val_str = Self::value_to_inline_string(a);
                                    output.push(format!("- {}\"{}\": {}{}",
                                        "  ".repeat(indent), key, val_str, comma).red().to_string());
                                }
                            }
                        }
                        (None, None) => unreachable!(),
                    }
                }

                output.push(format!("{}}}", indent_str));
            }
            (Value::Array(exp_arr), Value::Array(act_arr)) => {
                output.push("[".to_string());

                let max_len = exp_arr.len().max(act_arr.len());

                for i in 0..max_len {
                    let elem_path = format!("{}[{}]", current_path, i);
                    let exp_elem = exp_arr.get(i);
                    let act_elem = act_arr.get(i);
                    let is_last = i == max_len - 1;
                    let comma = if is_last { "" } else { "," };

                    match (exp_elem, act_elem) {
                        (Some(e), Some(a)) => {
                            if e == a {
                                // Same element
                                match e {
                                    Value::Object(_) | Value::Array(_) => {
                                        let val_str = Self::format_value_with_indent(e, indent + 1);
                                        let lines: Vec<&str> = val_str.lines().collect();
                                        // First line with indent
                                        output.push(format!("{}  {}", "  ".repeat(indent), lines[0]));
                                        // Subsequent lines already have proper indentation
                                        for (idx, line) in lines.iter().skip(1).enumerate() {
                                            let is_last_line = idx == lines.len() - 2;
                                            if is_last_line {
                                                output.push(format!("{}{}", line, comma));
                                            } else {
                                                output.push(line.to_string());
                                            }
                                        }
                                    }
                                    _ => {
                                        let val_str = Self::value_to_inline_string(e);
                                        output.push(format!("{}  {}{}",
                                            "  ".repeat(indent), val_str, comma));
                                    }
                                }
                            } else {
                                // Different - check if it's a complex type or leaf
                                match (e, a) {
                                    (Value::Object(_), Value::Object(_)) | (Value::Array(_), Value::Array(_)) => {
                                        // Recurse for complex types
                                        let nested_json = Self::format_unified_json(e, a, &elem_path, _diff_map, indent + 1);
                                        output.push(format!("{}  {}{}",
                                            "  ".repeat(indent), nested_json, comma));
                                    }
                                    _ => {
                                        // Leaf value difference - show inline
                                        let actual_str = Self::value_to_inline_string(a);
                                        let expected_str = Self::value_to_inline_string(e);

                                        output.push(format!("- {}{}{}",
                                            "  ".repeat(indent), actual_str, comma).red().to_string());
                                        output.push(format!("+ {}{}{}",
                                            "  ".repeat(indent), expected_str, comma).green().to_string());
                                    }
                                }
                            }
                        }
                        (Some(e), None) => {
                            // Missing in actual
                            match e {
                                Value::Object(_) | Value::Array(_) => {
                                    let val_str = Self::format_value_with_indent(e, indent);
                                    let lines: Vec<&str> = val_str.lines().collect();
                                    // First line with indent
                                    output.push(format!("+ {}{}", "  ".repeat(indent), lines[0]).green().to_string());
                                    // Subsequent lines: prefix at leftmost, line already has indentation
                                    for (idx, line) in lines.iter().skip(1).enumerate() {
                                        let is_last_line = idx == lines.len() - 2;
                                        if is_last_line {
                                            output.push(format!("+ {}{}", line, comma).green().to_string());
                                        } else {
                                            output.push(format!("+ {}", line).green().to_string());
                                        }
                                    }
                                }
                                _ => {
                                    let val_str = Self::value_to_inline_string(e);
                                    output.push(format!("+ {}{}{}",
                                        "  ".repeat(indent), val_str, comma).green().to_string());
                                }
                            }
                        }
                        (None, Some(a)) => {
                            // Extra in actual
                            match a {
                                Value::Object(_) | Value::Array(_) => {
                                    let val_str = Self::format_value_with_indent(a, indent);
                                    let lines: Vec<&str> = val_str.lines().collect();
                                    // First line with indent
                                    output.push(format!("- {}{}", "  ".repeat(indent), lines[0]).red().to_string());
                                    // Subsequent lines: prefix at leftmost, line already has indentation
                                    for (idx, line) in lines.iter().skip(1).enumerate() {
                                        let is_last_line = idx == lines.len() - 2;
                                        if is_last_line {
                                            output.push(format!("- {}{}", line, comma).red().to_string());
                                        } else {
                                            output.push(format!("- {}", line).red().to_string());
                                        }
                                    }
                                }
                                _ => {
                                    let val_str = Self::value_to_inline_string(a);
                                    output.push(format!("- {}{}{}",
                                        "  ".repeat(indent), val_str, comma).red().to_string());
                                }
                            }
                        }
                        (None, None) => unreachable!(),
                    }
                }

                output.push(format!("{}]", indent_str));
            }
            _ => {
                // Leaf values - should be handled by parent
                return Self::value_to_inline_string(expected);
            }
        }

        output.join("\n")
    }

    /// Convert a value to an inline string representation with proper indentation
    fn value_to_inline_string(value: &Value) -> String {
        // Use compact format for inline values - they'll be on the same line
        serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
    }

    /// Format a value with proper indentation for multi-line display
    fn format_value_with_indent(value: &Value, indent: usize) -> String {
        let indent_str = "  ".repeat(indent);
        match value {
            Value::Object(map) => {
                let mut output = vec!["{".to_string()];
                let keys: Vec<_> = map.keys().collect();
                for (i, key) in keys.iter().enumerate() {
                    if let Some(val) = map.get(*key) {
                        let is_last = i == keys.len() - 1;
                        let comma = if is_last { "" } else { "," };
                        let val_str = Self::value_to_inline_string(val);
                        output.push(format!("{}\"{}\": {}{}",
                            "  ".repeat(indent + 1), key, val_str, comma));
                    }
                }
                output.push(format!("{}}}", indent_str));
                output.join("\n")
            }
            Value::Array(arr) => {
                let mut output = vec!["[".to_string()];
                for (i, val) in arr.iter().enumerate() {
                    let is_last = i == arr.len() - 1;
                    let comma = if is_last { "" } else { "," };
                    let val_str = Self::value_to_inline_string(val);
                    output.push(format!("{}{}{}",
                        "  ".repeat(indent + 1), val_str, comma));
                }
                output.push(format!("{}]", indent_str));
                output.join("\n")
            }
            _ => Self::value_to_inline_string(value)
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
