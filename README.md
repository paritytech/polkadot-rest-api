# Polkadot REST API

## Benchmarks

### Benchmark Workflows

The repository includes two main benchmark workflows that run automatically on pushes and pull requests to the `main` branch:

#### 1. Benchmark workflow

- Builds and starts the server
- Runs performance tests against all configured endpoints
- Measures throughput (requests/second) and latency metrics (P50, P90, P99)
- Publishes results to GitHub Pages for historical tracking

**GitHub Pages Dashboard**: [https://paritytech.github.io/polkadot-rest-api/dev/bench/](https://paritytech.github.io/polkadot-rest-api/dev/bench/)

#### 2. Benchmark Comparison (vs Sidecar)


- Runs benchmarks against public Sidecar instance
- Calculates performance differences and improvements
- Generates comparison reports with percentage differences
- Publishes comparison metrics to GitHub Pages for trend analysis

**GitHub Pages Dashboard**: [https://paritytech.github.io/polkadot-rest-api/dev/bench/comparison/](https://paritytech.github.io/polkadot-rest-api/dev/bench/comparison/)

### Benchmark Metrics

Both workflows track the following metrics:

- **Throughput**: Requests per second (higher is better)
- **Average Latency**: Mean response time in milliseconds (lower is better)
- **P50 Latency**: 50th percentile latency (lower is better)
- **P90 Latency**: 90th percentile latency (lower is better)
- **P99 Latency**: 99th percentile latency (lower is better)
