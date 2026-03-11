# Benchmarks

Load testing suite for the polkadot-rest-api using [wrk](https://github.com/wg/wrk).

## Prerequisites

- [wrk](https://github.com/wg/wrk) installed
- [jq](https://jqlang.github.io/jq/) installed
- API server running (default: `http://localhost:8080`)

## Quick Start

```bash
# Run a single benchmark
./benchmarks/run.sh health

# Run with a specific scenario and hardware profile
./benchmarks/run.sh blocks_head medium_load dedicated_server

# Run all compatible benchmarks
./benchmarks/run.sh --all medium_load dedicated_server

# List available benchmarks
./benchmarks/run.sh
```

## run.sh

```
Usage: ./run.sh <benchmark_name> [scenario] [hardware_profile]
       ./run.sh --all [scenario] [hardware_profile] [results_dir]
```

### Scenarios

| Scenario | Threads | Connections | Duration | Best for |
|----------|---------|-------------|----------|----------|
| `light_load` | 2 | 10 | 30s | Development, CI |
| `medium_load` | 4 | 50 | 60s | General testing |
| `heavy_load` | 8 | 100 | 120s | Dedicated servers |
| `stress_test` | 12 | 200 | 300s | Finding breaking points |

### Hardware Profiles

| Profile | Recommended scenarios |
|---------|----------------------|
| `development` | light_load |
| `macbook` | light_load, medium_load |
| `ci_runner` | light_load, medium_load |
| `dedicated_server` | medium_load, heavy_load, stress_test |

### Chain-Aware Filtering

The runner auto-detects the connected chain by querying `/v1/capabilities`. Benchmarks that are incompatible with the detected chain type (relay, asset-hub, coretime, parachain) are automatically skipped.

### Results

Each benchmark run saves a JSON file to `results/` with metrics:

```json
{
  "endpoint": "blocks_head",
  "service": "polkadot-rest-api",
  "rps": 587.05,
  "avg_latency_ms": 85.08,
  "p50_ms": 75.90,
  "p90_ms": 99.61,
  "p95_ms": 120.50,
  "p99_ms": 301.12,
  "p999_ms": 450.00,
  "errors": 0,
  "total_requests": 35256,
  "duration_s": 60.00
}
```

Files are named `<benchmark>_<timestamp>.json` (e.g., `blocks_head_20260306_143022.json`).

## resource_monitor.sh

Monitors CPU and memory usage of the API process during benchmarks.

```
Usage: ./resource_monitor.sh [duration_minutes] [output_dir]
```

### Examples

```bash
# Monitor for 15 minutes (default)
./benchmarks/resource_monitor.sh

# Monitor for 5 minutes
./benchmarks/resource_monitor.sh 5

# Monitor for 30 minutes, custom output dir
./benchmarks/resource_monitor.sh 30 ~/results
```

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `MONITOR_PORT` | `8080` | Port to find the API process on |
| `MONITOR_PID` | _(auto-detect)_ | Skip port detection, monitor this PID directly |

### Output

- **Live**: RSS and CPU updated every second in terminal
- **CSV**: Saved to `results/resources_<process>_<timestamp>.csv` with columns: `timestamp, elapsed_s, rss_kb, vsz_kb, rss_mb, cpu_pct`
- **Summary**: Printed on exit (Ctrl+C or duration reached) with start/peak/end RSS, delta, avg/peak CPU

### Typical Workflow

Run the resource monitor in one terminal, the benchmark in another:

```bash
# Terminal 1: start monitoring
./benchmarks/resource_monitor.sh 5

# Terminal 2: run benchmark
./benchmarks/run.sh blocks_head medium_load dedicated_server

# When the benchmark finishes, Ctrl+C the monitor to see the summary
```

## compare.sh

Compare two benchmark runs side by side with percentage deltas.

```
Usage: ./compare.sh <file1.json> <file2.json> [resource1.csv] [resource2.csv]
```

### Examples

```bash
# Compare two runs (throughput + latency only)
./benchmarks/compare.sh results/blocks_head_20260306_100000.json results/blocks_head_20260306_110000.json

# Compare with resource data (adds memory + CPU)
./benchmarks/compare.sh \
  results/blocks_head_20260306_100000.json \
  results/blocks_head_20260306_110000.json \
  results/resources_polkadot-rest-api_20260306_100000.csv \
  results/resources_node_20260306_110000.csv

# Custom labels
LABEL_A="rest-api" LABEL_B="sidecar" ./benchmarks/compare.sh rust.json sidecar.json
```

### Output

```
==========================================
Benchmark Comparison
==========================================

  Endpoint A: blocks_head
  Endpoint B: blocks_head

                       rest-api         sidecar        Delta
  ----------------------------------------------------------------
  Throughput
    RPS                  587.05          203.42       -65.3%
    Total Requests        35256           12205
    Duration             60.00s          60.00s
  ----------------------------------------------------------------
  Latency
    Avg                 85.08ms        141.23ms       +66.0%
    P50                 75.90ms        135.10ms       +78.0%
    P90                 99.61ms        180.00ms       +80.7%
    P99                301.12ms        420.12ms       +39.5%
  ----------------------------------------------------------------
  Memory (RSS)
    Start                45.2MB          82.0MB       +81.4%
    Peak                 78.3MB         245.0MB      +213.0%
    End                  72.1MB         230.5MB      +219.7%
  ----------------------------------------------------------------
  CPU
    Avg                  12.5%           45.3%      +262.4%
    Peak                 35.0%           98.2%      +180.6%

  RPS:     positive delta = better (more throughput)
  Latency: negative delta = better (faster responses)
  Memory:  negative delta = better (less memory used)
  CPU:     negative delta = better (less CPU used)
==========================================
```

## Grafana Dashboard

A pre-built dashboard is auto-provisioned when running Grafana via docker compose.

The dashboard uses [process-exporter](https://github.com/ncabatoff/process-exporter) for per-process CPU and memory metrics.

### Setup — All in Docker (works on macOS and Linux)

Everything runs in Docker — API, process-exporter, Prometheus, Grafana. All Grafana panels populate including CPU/memory.

```bash
# Build the API image (first time only)
docker compose -f docker-compose.local.yml build

# Start everything
docker compose -f docker-compose.local.yml up -d

# Run benchmarks
./benchmarks/run.sh blocks_head medium_load dedicated_server

# Stop
docker compose -f docker-compose.local.yml down
```

Open http://localhost:3000 (admin/admin). The dashboard auto-loads.

### Setup — Native API + Docker monitoring (Linux only)

For benchmarking with native performance (no Docker overhead on the API). process-exporter requires Linux.

```bash
# Terminal 1 — API (native)
SAS_SUBSTRATE_URL=wss://rpc.polkadot.io SAS_METRICS_ENABLED=true \
  cargo run --release --bin polkadot-rest-api

# Terminal 2 — process-exporter (Linux only, exposes per-process CPU/memory on :9256)
process-exporter -config.path metrics/process-exporter.yml

# Terminal 3 — Prometheus + Grafana
docker network create monitoring 2>/dev/null
docker run -d --name prometheus --network monitoring -p 9090:9090 \
  -v $(pwd)/metrics/prometheus-local.yml:/etc/prometheus/prometheus.yml:ro \
  prom/prometheus:latest
docker run -d --name grafana --network monitoring -p 3000:3000 \
  -v $(pwd)/metrics/grafana/provisioning:/etc/grafana/provisioning:ro \
  -e GF_SECURITY_ADMIN_PASSWORD=admin \
  grafana/grafana:latest

# Terminal 4 — Run benchmarks
./benchmarks/run.sh blocks_head medium_load dedicated_server
```

**Stop:**
```bash
docker stop prometheus grafana && docker rm prometheus grafana
docker network rm monitoring
```

### Setup — macOS native API (no CPU/memory in Grafana)

On macOS without Docker for the API, process-exporter can't see native processes. Use `resource_monitor.sh` for CPU/memory instead.

```bash
# Terminal 1 — API
SAS_SUBSTRATE_URL=wss://rpc.polkadot.io SAS_METRICS_ENABLED=true \
  cargo run --release --bin polkadot-rest-api

# Terminal 2 — Prometheus + Grafana only
docker network create monitoring 2>/dev/null
docker run -d --name prometheus --network monitoring -p 9090:9090 \
  -v $(pwd)/metrics/prometheus-local.yml:/etc/prometheus/prometheus.yml:ro \
  prom/prometheus:latest
docker run -d --name grafana --network monitoring -p 3000:3000 \
  -v $(pwd)/metrics/grafana/provisioning:/etc/grafana/provisioning:ro \
  -e GF_SECURITY_ADMIN_PASSWORD=admin \
  grafana/grafana:latest

# Terminal 3 — Resource monitor (CPU/memory in terminal + CSV)
./benchmarks/resource_monitor.sh 15

# Terminal 4 — Run benchmarks
./benchmarks/run.sh blocks_head medium_load dedicated_server
```

### Dashboard Panels

| Row | Panels | Purpose |
|-----|--------|---------|
| **API Metrics** | Requests/sec, Request Duration (p50/p95/p99), Response Size | Real-time API performance |
| **By Route** | Requests by Route, P95 Latency by Route | Per-endpoint breakdown (use `$route` dropdown to filter) |
| **Process Resources** | Process CPU Usage, Process Memory (RSS) | Per-process CPU and memory via process-exporter |
| **Throughput vs Resources** | Throughput vs CPU, Throughput vs Memory, Latency vs Memory | Correlation panels to spot bottlenecks |
| **Saturation** | Error Rate vs Throughput, Latency Heatmap | Find breaking points |
| **Rate of Change** | Memory Growth Rate, I/O | Detect leaks and I/O bottlenecks |

## Configuration

All benchmark settings are in `benchmark_config.json` at the project root. This includes:

- Server host/port
- Hardware profiles and their allowed scenarios
- Scenario definitions (threads, connections, duration)
- Chain type mappings
- Per-benchmark chain compatibility

## Adding a New Benchmark

1. Create a directory under `benchmarks/` matching the benchmark name
2. Add a Lua script with the same name (e.g., `benchmarks/my_endpoint/my_endpoint.lua`)
3. Add an entry in `benchmark_config.json` under `"benchmarks"`
4. The Lua script should use `util.lua` for the `request()`, `done()`, and optionally `print_endpoints()` helpers
