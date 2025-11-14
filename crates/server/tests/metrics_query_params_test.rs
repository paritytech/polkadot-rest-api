/// Test that query parameters are properly included in route labels
/// when SAS_METRICS_INCLUDE_QUERYPARAMS is enabled
#[cfg(test)]
mod tests {
    #[test]
    fn test_normalize_route_without_query_params() {
        // Test basic path normalization without query params
        let result = normalize_route("/blocks/12345", None, false);
        assert_eq!(result, "/blocks/:blockId");

        let result = normalize_route("/blocks/0x1234abcd", None, false);
        assert_eq!(result, "/blocks/:blockId");
    }

    #[test]
    fn test_normalize_route_with_query_params_disabled() {
        // Even with query string, if disabled, don't include params
        let result = normalize_route(
            "/blocks/12345",
            Some("finalized=true&eventDocs=false"),
            false,
        );
        assert_eq!(result, "/blocks/:blockId");
    }

    #[test]
    fn test_normalize_route_with_query_params_enabled() {
        // With query params enabled, include them sorted alphabetically
        let result = normalize_route(
            "/blocks/12345",
            Some("finalized=true&eventDocs=false"),
            true,
        );
        // Should be sorted: eventDocs before finalized
        assert_eq!(result, "/blocks/:blockId?eventDocs=<?>&finalized=<?>");
    }

    #[test]
    fn test_normalize_route_query_params_alphabetical_sorting() {
        // Test that params are sorted alphabetically (matches sidecar)
        let result = normalize_route("/blocks/12345", Some("z_param=1&a_param=2&m_param=3"), true);
        assert_eq!(
            result,
            "/blocks/:blockId?a_param=<?>&m_param=<?>&z_param=<?>"
        );
    }

    #[test]
    fn test_normalize_route_empty_query_string() {
        // Empty query string should not add anything
        let result = normalize_route("/blocks/12345", Some(""), true);
        assert_eq!(result, "/blocks/:blockId");
    }

    #[test]
    fn test_normalize_route_single_param() {
        let result = normalize_route("/blocks/12345", Some("finalized=true"), true);
        assert_eq!(result, "/blocks/:blockId?finalized=<?>");
    }

    // Helper function to normalize routes (copy from middleware for testing)
    fn normalize_route(
        path: &str,
        query_string: Option<&str>,
        include_query_params: bool,
    ) -> String {
        let patterns = vec![
            (r"/blocks/[0-9]+$", "/blocks/:blockId"),
            (r"/blocks/0x[a-fA-F0-9]+$", "/blocks/:blockId"),
        ];

        let mut normalized = path.to_string();
        for (pattern, replacement) in patterns {
            if let Ok(re) = regex::Regex::new(pattern)
                && re.is_match(&normalized)
            {
                normalized = re.replace(&normalized, replacement).to_string();
                break;
            }
        }

        if include_query_params
            && let Some(query) = query_string
            && !query.is_empty()
        {
            let mut params: Vec<String> = query
                .split('&')
                .filter_map(|pair| pair.split('=').next().map(|name| name.to_string()))
                .collect();

            params.sort();

            let query_params = params
                .iter()
                .map(|name| format!("{}=<?>", name))
                .collect::<Vec<_>>()
                .join("&");

            normalized = format!("{}?{}", normalized, query_params);
        }

        normalized
    }
}
