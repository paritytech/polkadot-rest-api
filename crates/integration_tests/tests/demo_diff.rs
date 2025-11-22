use integration_tests::utils::compare_json;
use serde_json::json;

#[test]
fn test_colored_diff_demo() {
    // Expected JSON (what we want)
    let expected = json!({
        "specName": "polkadot",
        "specVersion": "9430",
        "authoringVersion": "0",
        "transactionVersion": "24",
        "properties": {
            "ss58Format": 0,
            "tokenSymbol": ["DOT"]
        },
        "extrinsics": [
            {"method": "transfer", "value": 100},
            {"method": "transferKeepAlive", "value": 200}
        ]
    });

    // Actual JSON (what we got - with intentional differences)
    let actual = json!({
        "specName": "polkadot-modified",  // Different value
        "specVersion": "9430",
        "authoringVersion": "2",          // Different value
        "transactionVersion": "24",
        "properties": {
            "ss58Format": 0,
            "tokenSymbol": ["DOT"],
            "extraField": "unexpected"    // Extra field
        },
        "extrinsics": [
            {"method": "transfer", "value": 100},
            {"method": "transferKeepAlive", "value": 300}, // Different value
            {"method": "bond", "value": 50}                 // Extra element
        ]
        // Note: "missingField" is missing
    });

    // Compare with no ignored fields
    let comparison = compare_json(&actual, &expected, &[]).unwrap();

    if !comparison.is_match() {
        // Print the colored diff
        println!("{}", comparison.format_diff(&expected, &actual));

        // Fail the test to show the output
        panic!("Intentional failure to demonstrate colored diff output");
    }
}
