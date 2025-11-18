use axum::{http::StatusCode, response::IntoResponse};

/// Handler for Prometheus metrics endpoint (text format)
pub async fn get_metrics() -> impl IntoResponse {
    match crate::metrics::gather_metrics() {
        Ok(metrics) => (
            StatusCode::OK,
            [("Content-Type", "text/plain; version=0.0.4")],
            metrics,
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to gather metrics: {}", e),
        )
            .into_response(),
    }
}

/// Handler for metrics in JSON format
pub async fn get_metrics_json() -> impl IntoResponse {
    use prometheus::proto::MetricFamily;
    use serde_json::json;

    let metric_families = match crate::metrics::gather_metric_families() {
        Ok(families) => families,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to gather metrics: {}", e),
            )
                .into_response();
        }
    };

    let metrics_json: Vec<serde_json::Value> = metric_families
        .iter()
        .map(|mf: &MetricFamily| {
            json!({
                "name": mf.get_name(),
                "help": mf.get_help(),
                "type": format!("{:?}", mf.get_field_type()),
                "metrics": mf.get_metric().iter().map(|m| {
                    json!({
                        "labels": m.get_label().iter().map(|l| {
                            json!({
                                "name": l.get_name(),
                                "value": l.get_value()
                            })
                        }).collect::<Vec<_>>(),
                        "value": if m.has_counter() {
                            json!(m.get_counter().get_value())
                        } else if m.has_histogram() {
                            let h = m.get_histogram();
                            json!({
                                "sample_count": h.get_sample_count(),
                                "sample_sum": h.get_sample_sum(),
                            })
                        } else {
                            json!(null)
                        }
                    })
                }).collect::<Vec<_>>()
            })
        })
        .collect();

    (StatusCode::OK, axum::Json(metrics_json)).into_response()
}
