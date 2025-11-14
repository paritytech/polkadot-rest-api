use lazy_static::lazy_static;
use prometheus::{
    Counter, Encoder, HistogramVec, Registry, TextEncoder, register_counter, register_histogram_vec,
};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new_custom(Some("sas".to_string()), None)
        .expect("Failed to create Prometheus registry");

    // Counter metrics
    pub static ref HTTP_REQUESTS: Counter = register_counter!(
        "http_requests",
        "Total number of HTTP requests"
    )
    .expect("Failed to create http_requests counter");

    pub static ref HTTP_REQUEST_SUCCESS: Counter = register_counter!(
        "http_request_success",
        "Number of successful HTTP requests"
    )
    .expect("Failed to create http_request_success counter");

    pub static ref HTTP_REQUEST_ERROR: Counter = register_counter!(
        "http_request_error",
        "Number of HTTP request errors"
    )
    .expect("Failed to create http_request_error counter");

    // Histogram metrics
    pub static ref REQUEST_DURATION_SECONDS: HistogramVec = register_histogram_vec!(
        "request_duration_seconds",
        "Duration of HTTP requests in seconds",
        &["method", "route", "status_code"],
        vec![0.1, 0.5, 1.0, 1.5, 2.0, 3.0, 4.0, 5.0]
    )
    .expect("Failed to create request_duration_seconds histogram");

    pub static ref RESPONSE_SIZE_BYTES: HistogramVec = register_histogram_vec!(
        "response_size_bytes",
        "Size of HTTP responses in bytes",
        &["method", "route", "status_code"],
        vec![100.0, 500.0, 1000.0, 5000.0, 10000.0, 50000.0, 100000.0, 500000.0, 1000000.0, 5000000.0]
    )
    .expect("Failed to create response_size_bytes histogram");

    pub static ref RESPONSE_SIZE_BYTES_SECONDS: HistogramVec = register_histogram_vec!(
        "response_size_bytes_seconds",
        "Ratio of response size to latency",
        &["method", "route", "status_code"],
        vec![64.0, 128.0, 256.0, 512.0, 1024.0, 2048.0, 4096.0, 8192.0, 16384.0, 32768.0, 65536.0, 131072.0, 262144.0]
    )
    .expect("Failed to create response_size_bytes_seconds histogram");

    pub static ref EXTRINSICS_IN_REQUEST: HistogramVec = register_histogram_vec!(
        "extrinsics_in_request",
        "Number of extrinsics in a request",
        &["method", "route", "status_code"],
        vec![5.0, 10.0, 20.0, 40.0, 80.0, 160.0, 320.0, 640.0, 1280.0, 2560.0, 5120.0, 10240.0, 20480.0]
    )
    .expect("Failed to create extrinsics_in_request histogram");

    pub static ref EXTRINSICS_PER_SECOND: HistogramVec = register_histogram_vec!(
        "extrinsics_per_second",
        "Number of extrinsics per second",
        &["method", "route", "status_code"],
        vec![1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0, 256.0, 512.0, 1024.0]
    )
    .expect("Failed to create extrinsics_per_second histogram");

    pub static ref EXTRINSICS_PER_BLOCK: HistogramVec = register_histogram_vec!(
        "extrinsics_per_block",
        "Average number of extrinsics per block",
        &["method", "route", "status_code"],
        vec![1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0, 256.0, 512.0, 1024.0]
    )
    .expect("Failed to create extrinsics_per_block histogram");

    pub static ref SECONDS_PER_BLOCK: HistogramVec = register_histogram_vec!(
        "seconds_per_block",
        "Average seconds per block",
        &["method", "route", "status_code"],
        vec![1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0, 256.0, 512.0, 1024.0]
    )
    .expect("Failed to create seconds_per_block histogram");
}

/// Initialize metrics by registering them with the custom registry
pub fn init() {
    use std::sync::Once;
    static INIT: Once = Once::new();

    // Only register metrics once
    INIT.call_once(|| {
        // Register all metrics with the custom registry
        REGISTRY
            .register(Box::new(HTTP_REQUESTS.clone()))
            .expect("Failed to register http_requests");

        REGISTRY
            .register(Box::new(HTTP_REQUEST_SUCCESS.clone()))
            .expect("Failed to register http_request_success");

        REGISTRY
            .register(Box::new(HTTP_REQUEST_ERROR.clone()))
            .expect("Failed to register http_request_error");

        REGISTRY
            .register(Box::new(REQUEST_DURATION_SECONDS.clone()))
            .expect("Failed to register request_duration_seconds");

        REGISTRY
            .register(Box::new(RESPONSE_SIZE_BYTES.clone()))
            .expect("Failed to register response_size_bytes");

        REGISTRY
            .register(Box::new(RESPONSE_SIZE_BYTES_SECONDS.clone()))
            .expect("Failed to register response_size_bytes_seconds");

        REGISTRY
            .register(Box::new(EXTRINSICS_IN_REQUEST.clone()))
            .expect("Failed to register extrinsics_in_request");

        REGISTRY
            .register(Box::new(EXTRINSICS_PER_SECOND.clone()))
            .expect("Failed to register extrinsics_per_second");

        REGISTRY
            .register(Box::new(EXTRINSICS_PER_BLOCK.clone()))
            .expect("Failed to register extrinsics_per_block");

        REGISTRY
            .register(Box::new(SECONDS_PER_BLOCK.clone()))
            .expect("Failed to register seconds_per_block");
    });
}

/// Gather all metrics as Prometheus text format
pub fn gather_metrics() -> Result<String, Box<dyn std::error::Error>> {
    let encoder = TextEncoder::new();
    let metric_families = REGISTRY.gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer)?;
    Ok(String::from_utf8(buffer)?)
}
