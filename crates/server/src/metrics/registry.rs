use lazy_static::lazy_static;
use prometheus::{Counter, Encoder, HistogramOpts, HistogramVec, Registry, TextEncoder};
use std::sync::{Mutex, Once};

lazy_static! {
    pub static ref REGISTRY: Mutex<Option<Registry>> = Mutex::new(None);
    static ref INIT_ONCE: Once = Once::new();

    // Counter metrics - created without registering to default registry
    pub static ref HTTP_REQUESTS: Counter = Counter::new(
        "http_requests",
        "Total number of HTTP requests"
    )
    .expect("Failed to create http_requests counter");

    pub static ref HTTP_REQUEST_SUCCESS: Counter = Counter::new(
        "http_request_success",
        "Number of successful HTTP requests"
    )
    .expect("Failed to create http_request_success counter");

    pub static ref HTTP_REQUEST_ERROR: Counter = Counter::new(
        "http_request_error",
        "Number of HTTP request errors"
    )
    .expect("Failed to create http_request_error counter");

    // Histogram metrics - created without registering to default registry
    pub static ref REQUEST_DURATION_SECONDS: HistogramVec = HistogramVec::new(
        HistogramOpts::new(
            "request_duration_seconds",
            "Duration of HTTP requests in seconds"
        ).buckets(vec![0.1, 0.5, 1.0, 1.5, 2.0, 3.0, 4.0, 5.0]),
        &["method", "route", "status_code"]
    )
    .expect("Failed to create request_duration_seconds histogram");

    pub static ref RESPONSE_SIZE_BYTES: HistogramVec = HistogramVec::new(
        HistogramOpts::new(
            "response_size_bytes",
            "Size of HTTP responses in bytes"
        ).buckets(vec![100.0, 500.0, 1000.0, 5000.0, 10000.0, 50000.0, 100000.0, 500000.0, 1000000.0, 5000000.0]),
        &["method", "route", "status_code"]
    )
    .expect("Failed to create response_size_bytes histogram");

    pub static ref RESPONSE_SIZE_BYTES_SECONDS: HistogramVec = HistogramVec::new(
        HistogramOpts::new(
            "response_size_bytes_seconds",
            "Ratio of response size to latency"
        ).buckets(vec![64.0, 128.0, 256.0, 512.0, 1024.0, 2048.0, 4096.0, 8192.0, 16384.0, 32768.0, 65536.0, 131072.0, 262144.0]),
        &["method", "route", "status_code"]
    )
    .expect("Failed to create response_size_bytes_seconds histogram");

    pub static ref EXTRINSICS_IN_REQUEST: HistogramVec = HistogramVec::new(
        HistogramOpts::new(
            "extrinsics_in_request",
            "Number of extrinsics in a request"
        ).buckets(vec![5.0, 10.0, 20.0, 40.0, 80.0, 160.0, 320.0, 640.0, 1280.0, 2560.0, 5120.0, 10240.0, 20480.0]),
        &["method", "route", "status_code"]
    )
    .expect("Failed to create extrinsics_in_request histogram");

    pub static ref EXTRINSICS_PER_SECOND: HistogramVec = HistogramVec::new(
        HistogramOpts::new(
            "extrinsics_per_second",
            "Number of extrinsics per second"
        ).buckets(vec![1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0, 256.0, 512.0, 1024.0]),
        &["method", "route", "status_code"]
    )
    .expect("Failed to create extrinsics_per_second histogram");

    pub static ref EXTRINSICS_PER_BLOCK: HistogramVec = HistogramVec::new(
        HistogramOpts::new(
            "extrinsics_per_block",
            "Average number of extrinsics per block"
        ).buckets(vec![1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0, 256.0, 512.0, 1024.0]),
        &["method", "route", "status_code"]
    )
    .expect("Failed to create extrinsics_per_block histogram");

    pub static ref SECONDS_PER_BLOCK: HistogramVec = HistogramVec::new(
        HistogramOpts::new(
            "seconds_per_block",
            "Average seconds per block"
        ).buckets(vec![1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0, 256.0, 512.0, 1024.0]),
        &["method", "route", "status_code"]
    )
    .expect("Failed to create seconds_per_block histogram");
}

/// Initialize metrics by registering them with the custom registry
pub fn init(prefix: &str) {
    // Only initialize once
    INIT_ONCE.call_once(|| {
        // Create registry with custom prefix
        let registry = Registry::new_custom(Some(prefix.to_string()), None)
            .expect("Failed to create Prometheus registry");

        // Register all metrics with the custom registry
        registry
            .register(Box::new(HTTP_REQUESTS.clone()))
            .expect("Failed to register http_requests");

        registry
            .register(Box::new(HTTP_REQUEST_SUCCESS.clone()))
            .expect("Failed to register http_request_success");

        registry
            .register(Box::new(HTTP_REQUEST_ERROR.clone()))
            .expect("Failed to register http_request_error");

        registry
            .register(Box::new(REQUEST_DURATION_SECONDS.clone()))
            .expect("Failed to register request_duration_seconds");

        registry
            .register(Box::new(RESPONSE_SIZE_BYTES.clone()))
            .expect("Failed to register response_size_bytes");

        registry
            .register(Box::new(RESPONSE_SIZE_BYTES_SECONDS.clone()))
            .expect("Failed to register response_size_bytes_seconds");

        registry
            .register(Box::new(EXTRINSICS_IN_REQUEST.clone()))
            .expect("Failed to register extrinsics_in_request");

        registry
            .register(Box::new(EXTRINSICS_PER_SECOND.clone()))
            .expect("Failed to register extrinsics_per_second");

        registry
            .register(Box::new(EXTRINSICS_PER_BLOCK.clone()))
            .expect("Failed to register extrinsics_per_block");

        registry
            .register(Box::new(SECONDS_PER_BLOCK.clone()))
            .expect("Failed to register seconds_per_block");

        // Store the registry
        *REGISTRY.lock().unwrap() = Some(registry);
    });
}

/// Gather all metrics as Prometheus text format
pub fn gather_metrics() -> Result<String, Box<dyn std::error::Error>> {
    let encoder = TextEncoder::new();
    let registry_guard = REGISTRY.lock().unwrap();
    let registry = registry_guard
        .as_ref()
        .expect("Metrics not initialized - call init() first");
    let metric_families = registry.gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer)?;
    Ok(String::from_utf8(buffer)?)
}
