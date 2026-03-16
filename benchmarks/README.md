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

## Modes Overview

| Mode | Command | What it does | What you get |
|------|---------|-------------|--------------|
| **Docker observability stack** | `docker compose -f docker-compose.local.yml up -d` | Runs the REST API + Prometheus + Grafana + process-exporter in containers | Live Grafana dashboards with API request metrics (latency, RPS, error rates) and process-level metrics (RSS, CPU) via process-exporter. Useful for continuous monitoring during development or manual exploratory testing. |
| **Standalone resource monitor** | `./benchmarks/resource_monitor.sh [port]` | Monitors the CPU and memory of the API process on a given port | Timestamped CSV with per-second RSS, VSZ, and CPU samples + summary (start/peak/end RSS, avg/peak CPU). Lightweight, no load generation — pair with your own traffic or manual testing. No dependencies beyond `ps`. |
| **Benchmark runner** | `./benchmarks/run.sh <name> <scenario> <hardware>` | Runs wrk-based HTTP load tests against specific endpoints | Human-readable wrk output (RPS, latency percentiles, transfer rates) + JSON results file. Auto-detects the connected chain and skips incompatible benchmarks. |
| **Benchmark with resource monitoring** | `./benchmarks/bench_monitored.sh <port> <name> <scenario> <hardware>` | Combines the benchmark runner with the resource monitor in a 3-phase run: baseline → load → cooldown | Both benchmark metrics (latency, throughput) and resource metrics (memory, CPU) in a single correlated run. Resource stats are merged into the benchmark JSON. Baseline and cooldown phases capture idle resource usage for comparison against load. |

## Docker Observability Stack

Runs the REST API, Prometheus, Grafana, and process-exporter in Docker containers. Provides live dashboards for API request metrics (latency, RPS, error rates) and process-level resource metrics (RSS, CPU).

### Prerequisites

- [Docker](https://docs.docker.com/get-docker/) installed

### Usage

```bash
# First time — build the API image
docker compose -f docker-compose.local.yml build

# Start rest-api & monitoring tools (prometheus, grafana, process-exporter)
docker compose -f docker-compose.local.yml up -d

# Open Grafana in browser
open http://localhost:3000    # admin / admin

# Run a benchmark (locally from your host)
./benchmarks/run.sh blocks light_load development

# Check Grafana update in real time during the benchmark
```

Configure the chain to test by editing the environment section in `docker-compose.local.yml`. For example, for Polkadot Asset Hub:

```yaml
environment:
  SAS_SUBSTRATE_MULTI_CHAIN_URL: '[{"url":"wss://rpc.polkadot.io","type":"relay"}]'
  SAS_SUBSTRATE_URL: wss://asset-hub-polkadot.dotters.network
  SAS_METRICS_ENABLED: "true"
```

### Stopping

```bash
# Stop all containers
docker compose -f docker-compose.local.yml down

# View past results offline (Prometheus data persists on disk)
docker compose -f docker-compose.local.yml up -d prometheus grafana
```

## Standalone resource monitor

Monitors CPU and memory usage of the API process during benchmarks or standalone use. Auto-detects the process listening on the given port.

```
Usage: ./resource_monitor.sh [port] [duration_minutes] [output_dir] [endpoint]
```

All arguments are optional.

| Argument | Default | Description |
|----------|---------|-------------|
| `port` | `8080` (or `MONITOR_PORT` env) | Port the API listens on |
| `duration_minutes` | `15` | Monitoring duration in minutes |
| `output_dir` | `../results` | Output directory |
| `endpoint` | `general` | Label for filenames and display. Set automatically by `bench_monitored.sh` to tag resource data per benchmark. |

### Examples

```bash
# Monitor port 8080 for 15 minutes (all defaults)
./benchmarks/resource_monitor.sh

# Monitor port 8080 for 5 minutes
./benchmarks/resource_monitor.sh 8080 5

# Custom output directory
./benchmarks/resource_monitor.sh 8080 15 ~/out

# With endpoint label (used by bench_monitored.sh)
./benchmarks/resource_monitor.sh 8080 15 ~/out blocks_head
```

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `MONITOR_PORT` | `8080` | Port to find the API process on (used when port arg is omitted) |
| `MONITOR_PID` | _(auto-detect)_ | Skip port detection, monitor this PID directly |

### Output

- **Live**: RSS and CPU updated every second in terminal
- **CSV**: Saved to `results/resources_<service>_<endpoint>_<timestamp>.csv` with columns: `timestamp, elapsed_s, rss_kb, vsz_kb, rss_mb, cpu_pct`
- **Summary**: Printed on exit (Ctrl+C or duration reached) with start/peak/end RSS, delta, avg/peak CPU

### Typical Workflow

Run the resource monitor in one terminal, the benchmark in another:

```bash
# Terminal 1: start monitoring
./benchmarks/resource_monitor.sh

# Terminal 2: run benchmark
./benchmarks/run.sh blocks_head medium_load dedicated_server

# When the benchmark finishes, Ctrl+C the monitor to see the summary
```

## Benchmark with resource monitoring

Runs a benchmark with resource monitoring in three phases: baseline (idle) → load (wrk benchmark, repeated N times) → cooldown (idle). Resource stats are merged into the benchmark JSON result file.

### Why three phases?

Running `run.sh` alone only gives you metrics during load. The 3-phase structure adds context:

- **Baseline** — records resting memory and CPU before any load hits, giving you the "before" snapshot.
- **Load** — runs the wrk benchmark while resource monitoring continues, capturing memory and CPU under stress. With `--runs N`, the load phase repeats N times back-to-back, each producing its own JSON result. Baseline and cooldown still run only once.
- **Cooldown** — shows whether memory drops back down after load stops. If RSS stays elevated after cooldown, that's a potential memory leak.

This also produces a **single JSON result file** (per run) with both throughput/latency and resource data merged together, instead of having to manually correlate separate wrk output and CSV files. With `--runs`, a summary JSON is also produced with min/max/avg across all runs.

```
Usage: ./bench_monitored.sh [--runs N] <port> <benchmark_name> <scenario> <hardware>
```

| Option | Default | Description |
|--------|---------|-------------|
| `--runs N` | 1 | Repeat the load phase N times. Baseline and cooldown run once. |

Baseline and cooldown durations scale with the scenario:

| Scenario | Total time (1 run) | Breakdown |
|----------|-----------|-----------|
| `light_load` | ~2.5 min | 1 min baseline + 30s load + 1 min cooldown |
| `medium_load` | ~3 min | 1 min baseline + 60s load + 1 min cooldown |
| `heavy_load` | ~4 min | 1 min baseline + 120s load + 1 min cooldown |
| `stress_test` | ~9 min | 2 min baseline + 300s load + 2 min cooldown |

### Examples

```bash
# Single run
./benchmarks/bench_monitored.sh 8080 blocks_head medium_load dedicated_server

# 5 load runs with shared baseline/cooldown
./benchmarks/bench_monitored.sh --runs 5 8080 blocks_head medium_load dedicated_server
```

### Tip: combine with Docker observability stack

Run `bench_monitored.sh` while the Docker stack is up to get the best of both: JSON/CSV result files for offline analysis and live Grafana dashboards to observe the impact in real time (latency spikes, memory growth, CPU saturation).

```bash
# Terminal 1: start the Docker stack (see Docker Observability Stack section)
docker compose -f docker-compose.local.yml up -d

# Terminal 2: run a monitored benchmark
./benchmarks/bench_monitored.sh 8080 blocks_head medium_load dedicated_server

# Watch Grafana at http://localhost:3000 during the baseline → load → cooldown phases
```

### Output

The JSON result file in `results/<benchmark>/` includes both wrk metrics and a `resources` section:

```json
{
  "endpoint": "blocks_head",
  "rps": 587.05,
  "p99_ms": 301.12,
  "resources": {
    "start_rss_mb": 45.2,
    "peak_rss_mb": 78.3,
    "end_rss_mb": 72.1,
    "delta_rss_mb": 26.9,
    "avg_cpu_pct": 12.5,
    "peak_cpu_pct": 35.0,
    "baseline_sec": 60,
    "cooldown_sec": 60
  }
}
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

All panels support the `$route` dropdown variable to filter by endpoint.

| Row | Panel | Description |
|-----|-------|-------------|
| **API Performance** | Requests/sec | Success and error request rates (1m window) |
| | Request Duration | Latency percentiles p50, p95, p99 (5m window) |
| | Response Size | Response body size percentiles p50, p95, p99 |
| | Requests by Route | Per-route request rate breakdown |
| | P95 Latency by Route | Per-route p95 latency comparison |
| | Error Rate vs Throughput | Overlays RPS with error rate to spot saturation |
| | Latency Heatmap | Distribution of request durations over time (log scale) |
| **Process Resources** | Process CPU Usage | Per-process CPU time (user/system mode) via process-exporter |
| | Process Memory (RSS) | Per-process resident set size |
| | Memory Growth Rate | `deriv()` of RSS — sustained positive values suggest a memory leak |
| | Network I/O | Per-process I/O read/write rates |
| **Correlation** | Throughput vs CPU | Dual-axis: RPS (left) overlaid with CPU usage (right) |
| | Throughput vs Memory | Dual-axis: RPS (left) overlaid with RSS (right) |
| | Latency vs Memory | Dual-axis: latency p50/p95/p99 (left) overlaid with RSS (right) — full width |

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
