#!/usr/bin/env bash
# Copyright (C) 2026 Parity Technologies (UK) Ltd.
# SPDX-License-Identifier: GPL-3.0-or-later

# Compare multiple benchmark runs from two tools, compute median/avg, generate HTML report
#
# Usage:
#   ./compare_runs.sh --a-dir results/rest-api/blocks_head --b-dir results/sidecar/blocks_head --scenario medium_load
#   ./compare_runs.sh --a results/*.json --b results/*.json --scenario light_load [--output report.html]
#
# Labels default to directory/file names, override with:
#   --label-a "polkadot-rest-api" --label-b "substrate-api-sidecar"
#
# Requires: jq, awk

set -euo pipefail

# Force C locale so awk/sort always use '.' as decimal separator (not ',' from e.g. French locale)
export LC_ALL=C

# --- Parse arguments ---
FILES_A=()
FILES_B=()
LABEL_A="${LABEL_A:-}"
LABEL_B="${LABEL_B:-}"
REPORTS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/reports"
OUTPUT="$REPORTS_DIR/comparison_report.html"
SCENARIO_FILTER=""

parse_args() {
    local mode=""
    while [ $# -gt 0 ]; do
        case "$1" in
            --a)       mode="a"; shift ;;
            --b)       mode="b"; shift ;;
            --a-dir)
                shift
                for f in "$1"/*.json; do [ -f "$f" ] && FILES_A+=("$f"); done
                mode=""; shift ;;
            --b-dir)
                shift
                for f in "$1"/*.json; do [ -f "$f" ] && FILES_B+=("$f"); done
                mode=""; shift ;;
            --scenario) shift; SCENARIO_FILTER="$1"; shift ;;
            --label-a) shift; LABEL_A="$1"; shift ;;
            --label-b) shift; LABEL_B="$1"; shift ;;
            --output)  shift; OUTPUT="$1"; shift ;;
            --help|-h)
                echo "Usage: $0 --a-dir DIR_A --b-dir DIR_B --scenario SCENARIO [options]"
                echo "       $0 --a file1.json ... --b file1.json ... --scenario SCENARIO [options]"
                echo ""
                echo "Required:"
                echo "  --scenario NAME     Scenario to compare (light_load, medium_load, heavy_load, stress_test)"
                echo ""
                echo "Options:"
                echo "  --a FILE ...        Result JSON files for tool A"
                echo "  --b FILE ...        Result JSON files for tool B"
                echo "  --a-dir DIR         Directory of JSON files for tool A"
                echo "  --b-dir DIR         Directory of JSON files for tool B"
                echo "  --label-a NAME      Label for tool A (default: from JSON service field)"
                echo "  --label-b NAME      Label for tool B (default: from JSON service field)"
                echo "  --output FILE       Output HTML file (default: comparison_<endpoint>_<scenario>.html)"
                exit 0 ;;
            *)
                if [ "$mode" = "a" ]; then
                    FILES_A+=("$1")
                elif [ "$mode" = "b" ]; then
                    FILES_B+=("$1")
                else
                    echo "Error: unexpected argument '$1'. Use --a or --b to specify files."
                    exit 1
                fi
                shift ;;
        esac
    done
}

parse_args "$@"

if [ ${#FILES_A[@]} -eq 0 ] || [ ${#FILES_B[@]} -eq 0 ]; then
    echo "Error: Need at least one file for each tool (--a and --b)"
    echo "Run $0 --help for usage"
    exit 1
fi

# Validate files exist
for f in "${FILES_A[@]}" "${FILES_B[@]}"; do
    if [ ! -f "$f" ]; then
        echo "Error: File not found: $f"
        exit 1
    fi
done

# --scenario is required
if [ -z "$SCENARIO_FILTER" ]; then
    echo "Error: --scenario is required (e.g. --scenario medium_load)"
    echo ""
    echo "Available scenarios in A:"
    for f in "${FILES_A[@]}"; do jq -r '.scenario // "unknown"' "$f" 2>/dev/null; done | sort -u | sed 's/^/  /'
    echo "Available scenarios in B:"
    for f in "${FILES_B[@]}"; do jq -r '.scenario // "unknown"' "$f" 2>/dev/null; done | sort -u | sed 's/^/  /'
    exit 1
fi

# Filter files by scenario
FILTERED_A=()
for f in "${FILES_A[@]}"; do
    s=$(jq -r '.scenario // ""' "$f" 2>/dev/null)
    [ "$s" = "$SCENARIO_FILTER" ] && FILTERED_A+=("$f")
done
FILTERED_B=()
for f in "${FILES_B[@]}"; do
    s=$(jq -r '.scenario // ""' "$f" 2>/dev/null)
    [ "$s" = "$SCENARIO_FILTER" ] && FILTERED_B+=("$f")
done
if [ ${#FILTERED_A[@]} -eq 0 ] || [ ${#FILTERED_B[@]} -eq 0 ]; then
    echo "Error: No runs match scenario '$SCENARIO_FILTER'"
    [ ${#FILTERED_A[@]} -eq 0 ] && echo "  Tool A: 0 matching files (had ${#FILES_A[@]} total)"
    [ ${#FILTERED_B[@]} -eq 0 ] && echo "  Tool B: 0 matching files (had ${#FILES_B[@]} total)"
    echo ""
    echo "Available scenarios in A:"
    for f in "${FILES_A[@]}"; do jq -r '.scenario // "unknown"' "$f" 2>/dev/null; done | sort -u | sed 's/^/  /'
    echo "Available scenarios in B:"
    for f in "${FILES_B[@]}"; do jq -r '.scenario // "unknown"' "$f" 2>/dev/null; done | sort -u | sed 's/^/  /'
    exit 1
fi
FILES_A=("${FILTERED_A[@]}")
FILES_B=("${FILTERED_B[@]}")
echo "Scenario: $SCENARIO_FILTER (A=${#FILES_A[@]} runs, B=${#FILES_B[@]} runs)"

# Default labels from first file's service field
if [ -z "$LABEL_A" ]; then
    LABEL_A=$(jq -r '.service // "Tool A"' "${FILES_A[0]}")
fi
if [ -z "$LABEL_B" ]; then
    LABEL_B=$(jq -r '.service // "Tool B"' "${FILES_B[0]}")
fi

ENDPOINT=$(jq -r '.endpoint // "unknown"' "${FILES_A[0]}")

echo "Comparing: $LABEL_A (${#FILES_A[@]} runs) vs $LABEL_B (${#FILES_B[@]} runs)"
echo "Endpoint:  $ENDPOINT"
echo ""

# --- Stats computation ---

# Extract a field from all files, one value per line
extract_field() {
    local field="$1"
    shift
    for f in "$@"; do
        jq -r ".$field // 0" "$f"
    done
}

# Compute median from a list of values (one per line on stdin)
median() {
    sort -n | awk '{a[NR]=$1} END {
        if (NR%2==1) printf "%.4f", a[(NR+1)/2]
        else printf "%.4f", (a[NR/2] + a[NR/2+1]) / 2
    }'
}

# Compute average from a list of values
average() {
    awk '{sum+=$1; n++} END { if(n>0) printf "%.4f", sum/n; else print 0 }'
}

# Compute min
minimum() {
    sort -n | head -1
}

# Compute max
maximum() {
    sort -n | tail -1
}

# Compute stdev
stdev() {
    awk '{sum+=$1; sumsq+=$1*$1; n++} END {
        if(n<2) { printf "0"; exit }
        mean=sum/n
        var = (sumsq - n*mean*mean)/(n-1)
        printf "%.4f", sqrt(var < 0 ? 0 : var)
    }'
}

# Get all stats for a field across files
compute_stats() {
    local field="$1"
    shift
    local values
    values=$(extract_field "$field" "$@")
    local med avg mn mx sd
    med=$(echo "$values" | median)
    avg=$(echo "$values" | average)
    mn=$(echo "$values" | minimum)
    mx=$(echo "$values" | maximum)
    sd=$(echo "$values" | stdev)
    echo "$med $avg $mn $mx $sd"
}

# Fields to compare
FIELDS="rps avg_latency_ms stdev_ms max_latency_ms min_latency_ms p50_ms p75_ms p90_ms p95_ms p99_ms p999_ms errors_total total_requests duration_s bytes transfer_per_sec req_sec_avg"

# Resource fields (from resources object merged by bench_with_monitor.sh)
RESOURCE_FIELDS="resources.start_rss_mb resources.peak_rss_mb resources.end_rss_mb resources.delta_rss_mb resources.avg_cpu_pct resources.peak_cpu_pct"

# Check if resource data exists in the JSON files
HAS_RESOURCES_A=$(jq -r '.resources.peak_rss_mb // empty' "${FILES_A[0]}" 2>/dev/null)
HAS_RESOURCES_B=$(jq -r '.resources.peak_rss_mb // empty' "${FILES_B[0]}" 2>/dev/null)
HAS_RESOURCES=""
if [ -n "$HAS_RESOURCES_A" ] && [ -n "$HAS_RESOURCES_B" ]; then
    HAS_RESOURCES="true"
fi

# Compute stats for both tools using temp files (bash 3.2 compatible — no associative arrays)
STATS_DIR=$(mktemp -d)
trap "rm -rf $STATS_DIR" EXIT
for field in $FIELDS; do
    compute_stats "$field" "${FILES_A[@]}" > "$STATS_DIR/a_$field"
    compute_stats "$field" "${FILES_B[@]}" > "$STATS_DIR/b_$field"
done

# Compute resource stats if available
if [ -n "$HAS_RESOURCES" ]; then
    for field in $RESOURCE_FIELDS; do
        safe_name=$(echo "$field" | tr '.' '_')
        compute_stats "$field" "${FILES_A[@]}" > "$STATS_DIR/a_$safe_name"
        compute_stats "$field" "${FILES_B[@]}" > "$STATS_DIR/b_$safe_name"
    done
fi

# Helper to extract stats from temp files
get_stat() { awk -v pos="$1" '{print $pos}' "$STATS_DIR/${2}_${3}"; }
get_median() { get_stat 1 "$1" "$2"; }
get_avg()    { get_stat 2 "$1" "$2"; }
get_min()    { get_stat 3 "$1" "$2"; }
get_max()    { get_stat 4 "$1" "$2"; }
get_stdev()  { get_stat 5 "$1" "$2"; }

# --- Terminal output ---

pct() {
    awk -v a="$1" -v b="$2" 'BEGIN {
        if (a+0 == 0) printf "N/A"
        else printf "%+.1f%%", ((b - a) / a) * 100
    }'
}

fmt() { awk -v v="$1" -v u="${2:-}" 'BEGIN { printf "%.2f%s", v, u }'; }

echo "=========================================="
echo "Benchmark Comparison (Median of N runs)"
echo "=========================================="
echo ""
printf "  %-20s %15s %15s %12s\n" "" "$LABEL_A" "$LABEL_B" "Delta"
echo "  ----------------------------------------------------------------"
echo "  Throughput"
A_MED=$(get_median "a" "rps"); B_MED=$(get_median "b" "rps")
printf "  %-20s %12s %15s %12s\n" "  RPS (median)" "$(fmt "$A_MED")" "$(fmt "$B_MED")" "$(pct "$A_MED" "$B_MED")"
A_AVG_V=$(get_avg "a" "rps"); B_AVG_V=$(get_avg "b" "rps")
printf "  %-20s %12s %15s %12s\n" "  RPS (avg)"    "$(fmt "$A_AVG_V")" "$(fmt "$B_AVG_V")" "$(pct "$A_AVG_V" "$B_AVG_V")"
echo "  ----------------------------------------------------------------"
echo "  Latency (median of runs)"
for field in avg_latency_ms p50_ms p75_ms p90_ms p95_ms p99_ms p999_ms; do
    label=$(echo "$field" | sed 's/_ms//;s/avg_latency/Avg/;s/p999/P99.9/;s/p99/P99/;s/p95/P95/;s/p90/P90/;s/p75/P75/;s/p50/P50/')
    A_MED=$(get_median "a" "$field"); B_MED=$(get_median "b" "$field")
    printf "  %-20s %12s %15s %12s\n" "  $label" "$(fmt "$A_MED" ms)" "$(fmt "$B_MED" ms)" "$(pct "$A_MED" "$B_MED")"
done
echo "  ----------------------------------------------------------------"
echo "  Runs: $LABEL_A=${#FILES_A[@]}, $LABEL_B=${#FILES_B[@]}"
echo "=========================================="
echo ""

# --- Generate HTML ---

# Collect all individual run values for scatter/box charts
ALL_RPS_A=$(extract_field "rps" "${FILES_A[@]}" | tr '\n' ',' | sed 's/,$//')
ALL_RPS_B=$(extract_field "rps" "${FILES_B[@]}" | tr '\n' ',' | sed 's/,$//')
ALL_AVG_A=$(extract_field "avg_latency_ms" "${FILES_A[@]}" | tr '\n' ',' | sed 's/,$//')
ALL_AVG_B=$(extract_field "avg_latency_ms" "${FILES_B[@]}" | tr '\n' ',' | sed 's/,$//')
ALL_P99_A=$(extract_field "p99_ms" "${FILES_A[@]}" | tr '\n' ',' | sed 's/,$//')
ALL_P99_B=$(extract_field "p99_ms" "${FILES_B[@]}" | tr '\n' ',' | sed 's/,$//')
ALL_ERRORS_A=$(extract_field "errors_total" "${FILES_A[@]}" | tr '\n' ',' | sed 's/,$//')
ALL_ERRORS_B=$(extract_field "errors_total" "${FILES_B[@]}" | tr '\n' ',' | sed 's/,$//')
ALL_TOTAL_REQS_A=$(extract_field "total_requests" "${FILES_A[@]}" | tr '\n' ',' | sed 's/,$//')
ALL_TOTAL_REQS_B=$(extract_field "total_requests" "${FILES_B[@]}" | tr '\n' ',' | sed 's/,$//')

# Median values for charts
M_A_RPS=$(get_median "a" "rps")
M_B_RPS=$(get_median "b" "rps")
M_A_AVG=$(get_median "a" "avg_latency_ms")
M_B_AVG=$(get_median "b" "avg_latency_ms")
M_A_P50=$(get_median "a" "p50_ms")
M_B_P50=$(get_median "b" "p50_ms")
M_A_P75=$(get_median "a" "p75_ms")
M_B_P75=$(get_median "b" "p75_ms")
M_A_P90=$(get_median "a" "p90_ms")
M_B_P90=$(get_median "b" "p90_ms")
M_A_P95=$(get_median "a" "p95_ms")
M_B_P95=$(get_median "b" "p95_ms")
M_A_P99=$(get_median "a" "p99_ms")
M_B_P99=$(get_median "b" "p99_ms")
M_A_P999=$(get_median "a" "p999_ms")
M_B_P999=$(get_median "b" "p999_ms")

# Average values
V_A_RPS=$(get_avg "a" "rps")
V_B_RPS=$(get_avg "b" "rps")
V_A_AVG=$(get_avg "a" "avg_latency_ms")
V_B_AVG=$(get_avg "b" "avg_latency_ms")

# Stdev of runs
SD_A_RPS=$(get_stdev "a" "rps")
SD_B_RPS=$(get_stdev "b" "rps")
SD_A_AVG=$(get_stdev "a" "avg_latency_ms")
SD_B_AVG=$(get_stdev "b" "avg_latency_ms")
SD_A_P99=$(get_stdev "a" "p99_ms")
SD_B_P99=$(get_stdev "b" "p99_ms")

# Request duration and response size
M_A_DUR=$(get_median "a" "duration_s")
M_B_DUR=$(get_median "b" "duration_s")
M_A_REQS=$(get_median "a" "total_requests")
M_B_REQS=$(get_median "b" "total_requests")
M_A_BYTES=$(get_median "a" "bytes")
M_B_BYTES=$(get_median "b" "bytes")
M_A_TRANSFER=$(get_median "a" "transfer_per_sec")
M_B_TRANSFER=$(get_median "b" "transfer_per_sec")
M_A_MINLAT=$(get_median "a" "min_latency_ms")
M_B_MINLAT=$(get_median "b" "min_latency_ms")
M_A_MAXLAT=$(get_median "a" "max_latency_ms")
M_B_MAXLAT=$(get_median "b" "max_latency_ms")
M_A_STDLAT=$(get_median "a" "stdev_ms")
M_B_STDLAT=$(get_median "b" "stdev_ms")
M_A_ERRORS=$(get_median "a" "errors_total")
M_B_ERRORS=$(get_median "b" "errors_total")

# Resource data for charts
if [ -n "$HAS_RESOURCES" ]; then
    M_A_START_RSS=$(get_median "a" "resources_start_rss_mb")
    M_B_START_RSS=$(get_median "b" "resources_start_rss_mb")
    M_A_PEAK_RSS=$(get_median "a" "resources_peak_rss_mb")
    M_B_PEAK_RSS=$(get_median "b" "resources_peak_rss_mb")
    M_A_END_RSS=$(get_median "a" "resources_end_rss_mb")
    M_B_END_RSS=$(get_median "b" "resources_end_rss_mb")
    M_A_DELTA_RSS=$(get_median "a" "resources_delta_rss_mb")
    M_B_DELTA_RSS=$(get_median "b" "resources_delta_rss_mb")
    M_A_AVG_CPU=$(get_median "a" "resources_avg_cpu_pct")
    M_B_AVG_CPU=$(get_median "b" "resources_avg_cpu_pct")
    M_A_PEAK_CPU=$(get_median "a" "resources_peak_cpu_pct")
    M_B_PEAK_CPU=$(get_median "b" "resources_peak_cpu_pct")

    ALL_PEAK_RSS_A=$(extract_field "resources.peak_rss_mb" "${FILES_A[@]}" | tr '\n' ',' | sed 's/,$//')
    ALL_PEAK_RSS_B=$(extract_field "resources.peak_rss_mb" "${FILES_B[@]}" | tr '\n' ',' | sed 's/,$//')
    ALL_AVG_CPU_A=$(extract_field "resources.avg_cpu_pct" "${FILES_A[@]}" | tr '\n' ',' | sed 's/,$//')
    ALL_AVG_CPU_B=$(extract_field "resources.avg_cpu_pct" "${FILES_B[@]}" | tr '\n' ',' | sed 's/,$//')
fi

# Scenario info from first file
SCENARIO_A=$(jq -r '.scenario // "unknown"' "${FILES_A[0]}")
THREADS_A=$(jq -r '.threads // "?"' "${FILES_A[0]}")
CONNS_A=$(jq -r '.connections // "?"' "${FILES_A[0]}")
CHAIN_A=$(jq -r '.chain // "unknown"' "${FILES_A[0]}")

# Build output filename from endpoint and scenario if not explicitly set
if [ "$OUTPUT" = "$REPORTS_DIR/comparison_report.html" ]; then
    ENDPOINT_SLUG=$(echo "$ENDPOINT" | tr '/' '_' | sed 's/^_//')
    OUTPUT="$REPORTS_DIR/comparison_${ENDPOINT_SLUG}_${SCENARIO_A}.html"
fi
mkdir -p "$(dirname "$OUTPUT")"

cat > "$OUTPUT" <<'HTMLEOF'
<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<title>Benchmark Report: LABEL_A_PH vs LABEL_B_PH</title>
<script src="https://cdn.jsdelivr.net/npm/chart.js@4"></script>
<style>
  * { margin: 0; padding: 0; box-sizing: border-box; }
  body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, monospace;
         background: #0d1117; color: #c9d1d9; padding: 24px; max-width: 1400px; margin: 0 auto; }
  h1 { text-align: center; margin-bottom: 8px; font-size: 1.5em; color: #e6edf3; }
  .hero { text-align: center; margin-bottom: 20px; }
  .hero .endpoint { font-size: 1.3em; font-weight: 700; color: #58a6ff; margin-bottom: 6px; font-family: monospace; }
  .hero .chain-scenario { font-size: 1.05em; color: #e6edf3; margin-bottom: 6px; }
  .hero .chain-scenario .chain { color: #3fb950; font-weight: 600; }
  .hero .chain-scenario .scenario { color: #d29922; font-weight: 600; }
  .subtitle { text-align: center; color: #8b949e; margin-bottom: 24px; font-size: 0.85em; }
  .grid { display: grid; grid-template-columns: 1fr 1fr; gap: 20px; margin-bottom: 20px; }
  .card { background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 16px; }
  .card.full { grid-column: 1 / -1; }
  .card h2 { font-size: 1em; color: #e6edf3; margin-bottom: 4px; }
  .card .insight { font-size: 0.8em; color: #8b949e; margin-bottom: 12px; line-height: 1.5; }
  .card .insight .better { color: #3fb950; font-weight: 600; }
  .card .insight .worse { color: #f85149; font-weight: 600; }
  .card .insight .neutral { color: #d29922; font-weight: 600; }
  canvas { width: 100% !important; }
  .summary { display: grid; grid-template-columns: repeat(4, 1fr); gap: 12px; margin-bottom: 20px; }
  .stat { background: #161b22; border: 1px solid #30363d; border-radius: 8px; padding: 14px; text-align: center; }
  .stat .label { font-size: 0.7em; color: #8b949e; text-transform: uppercase; letter-spacing: 0.05em; }
  .stat .value { font-size: 1.4em; font-weight: 700; margin-top: 4px; }
  .stat .detail { font-size: 0.7em; color: #8b949e; margin-top: 2px; }
  .better { color: #3fb950; }
  .worse { color: #f85149; }
  .neutral { color: #c9d1d9; }
  .variance-note { background: #1c2128; border: 1px solid #30363d; border-radius: 8px;
                   padding: 16px; margin-bottom: 20px; font-size: 0.85em; line-height: 1.6; }
  .variance-note h3 { color: #d29922; font-size: 0.95em; margin-bottom: 8px; }
  .glossary { background: #1c2128; border: 1px solid #30363d; border-radius: 8px;
              padding: 20px; margin: 28px 0 20px 0; font-size: 0.85em; line-height: 1.7; }
  .glossary h3 { color: #d29922; font-size: 1em; margin-bottom: 12px; }
  .glossary h4 { color: #58a6ff; font-size: 0.95em; margin: 16px 0 6px 0; }
  .glossary h4:first-of-type { margin-top: 0; }
  .glossary p { margin: 4px 0; }
  .glossary .back-link { font-size: 0.8em; color: #8b949e; }
  .glossary .back-link a { color: #58a6ff; text-decoration: none; }
  .glossary .back-link a:hover { text-decoration: underline; }
  .learn-more { font-size: 0.75em; margin-left: 6px; }
  .learn-more a { color: #58a6ff; text-decoration: none; border-bottom: 1px dotted #58a6ff; padding-bottom: 1px; }
  .learn-more a:hover { color: #79c0ff; border-bottom-color: #79c0ff; }
  .section-title { color: #8b949e; font-size: 0.85em; text-transform: uppercase; letter-spacing: 0.1em;
                    border-bottom: 1px solid #30363d; padding-bottom: 6px; margin: 28px 0 16px 0; }
  .section-title:first-of-type { margin-top: 0; }
  .meta { text-align: center; color: #484f58; font-size: 0.75em; margin-top: 20px; }
</style>
</head>
<body>
HTMLEOF

# Now inject actual values using sed
if [[ "$OSTYPE" == "darwin"* ]]; then
    sed -i '' "s/LABEL_A_PH/$LABEL_A/g; s/LABEL_B_PH/$LABEL_B/g" "$OUTPUT"
else
    sed -i "s/LABEL_A_PH/$LABEL_A/g; s/LABEL_B_PH/$LABEL_B/g" "$OUTPUT"
fi

cat >> "$OUTPUT" <<EOF

<h1>Benchmark Report: ${LABEL_A} vs ${LABEL_B}</h1>
<div class="hero">
  <div class="endpoint">${ENDPOINT}</div>
  <div class="chain-scenario">
    Chain: <span class="chain">${CHAIN_A}</span> &nbsp;&bull;&nbsp;
    Load: <span class="scenario">${SCENARIO_A}</span>
    (${THREADS_A}t / ${CONNS_A}c)
  </div>
</div>
<p class="subtitle">
  Runs: ${#FILES_A[@]} vs ${#FILES_B[@]} |
  $(date +%Y-%m-%d\ %H:%M)
</p>

<div class="summary">
  <div class="stat">
    <div class="label">Runs - ${LABEL_A}</div>
    <div class="value neutral">${#FILES_A[@]}</div>
    <div class="detail">benchmark executions</div>
  </div>
  <div class="stat">
    <div class="label">Runs - ${LABEL_B}</div>
    <div class="value neutral">${#FILES_B[@]}</div>
    <div class="detail">benchmark executions</div>
  </div>
  <div class="stat">
    <div class="label">Median RPS - ${LABEL_A}</div>
    <div class="value neutral">${M_A_RPS}</div>
    <div class="detail">avg: ${V_A_RPS} | stdev: ${SD_A_RPS}</div>
  </div>
  <div class="stat">
    <div class="label">Median RPS - ${LABEL_B}</div>
    <div class="value neutral">${M_B_RPS}</div>
    <div class="detail">avg: ${V_B_RPS} | stdev: ${SD_B_RPS}</div>
  </div>
  <div class="stat">
    <div class="label">Median Latency - ${LABEL_A}</div>
    <div class="value neutral">${M_A_AVG}ms</div>
    <div class="detail">avg: ${V_A_AVG}ms | stdev: ${SD_A_AVG}ms</div>
  </div>
  <div class="stat">
    <div class="label">Median Latency - ${LABEL_B}</div>
    <div class="value neutral">${M_B_AVG}ms</div>
    <div class="detail">avg: ${V_B_AVG}ms | stdev: ${SD_B_AVG}ms</div>
  </div>
EOF

if [ -n "$HAS_RESOURCES" ]; then
cat >> "$OUTPUT" <<EOF
  <div class="stat">
    <div class="label">Peak RSS - ${LABEL_A}</div>
    <div class="value neutral">${M_A_PEAK_RSS} MB</div>
    <div class="detail">start: ${M_A_START_RSS} MB | delta: ${M_A_DELTA_RSS} MB</div>
  </div>
  <div class="stat">
    <div class="label">Peak RSS - ${LABEL_B}</div>
    <div class="value neutral">${M_B_PEAK_RSS} MB</div>
    <div class="detail">start: ${M_B_START_RSS} MB | delta: ${M_B_DELTA_RSS} MB</div>
  </div>
EOF
fi

cat >> "$OUTPUT" <<EOF
</div>

<h3 class="section-title">API Performance</h3>
<div class="grid">

  <div class="card">
    <h2>RPS (Throughput) - higher is better</h2>
    <div class="insight" id="rpsInsight"></div>
    <canvas id="rpsChart"></canvas>
  </div>

  <div class="card">
    <h2>RPS per Run - higher &amp; tighter is better</h2>
    <div class="insight" id="rpsRunInsight"></div>
    <canvas id="rpsRunChart"></canvas>
  </div>

  <div class="card" id="card-duration">
    <h2>Request Duration (Median) - lower is better <span class="learn-more"><a href="#glossary-stdev">stdev &#x2193;</a> | <a href="#glossary-cv">CV &#x2193;</a></span></h2>
    <div class="insight" id="durationInsight"></div>
    <canvas id="durationChart"></canvas>
  </div>

  <div class="card" id="card-max-duration">
    <h2>Max Request Duration - single slowest request <span class="learn-more"><a href="#glossary-max">glossary &#x2193;</a></span></h2>
    <div class="insight" id="maxDurationInsight"></div>
    <canvas id="maxDurationChart"></canvas>
  </div>

  <div class="card">
    <h2>Transfer Rate - higher is better</h2>
    <div class="insight" id="transferInsight"></div>
    <canvas id="transferChart"></canvas>
  </div>

  <div class="card">
    <h2>Avg Response Size - similar means apples-to-apples</h2>
    <div class="insight" id="respSizeInsight"></div>
    <canvas id="respSizeChart"></canvas>
  </div>

  <div class="card">
    <h2>Latency Percentiles (Median) - lower is better</h2>
    <div class="insight" id="latencyInsight"></div>
    <canvas id="latencyChart"></canvas>
  </div>

  <div class="card">
    <h2>P99 Latency per Run - lower &amp; tighter is better</h2>
    <div class="insight" id="p99Insight"></div>
    <canvas id="p99Chart"></canvas>
  </div>

  <div class="card">
    <h2>Latency Distribution (Median) - lower is better</h2>
    <p style="font-size:0.75em; color:#8b949e; margin-bottom:8px;">
      How latency grows from typical (P50) to worst case (P99.9).
      Flatter = more consistent. Steeper = worse tail latency.
    </p>
    <div class="insight" id="distInsight"></div>
    <canvas id="distChart"></canvas>
  </div>

  <div class="card">
    <h2>Error Rate per Run - lower is better (0 is ideal)</h2>
    <div class="insight" id="errorInsight"></div>
    <canvas id="errorChart"></canvas>
  </div>

</div>

EOF

if [ -n "$HAS_RESOURCES" ]; then
cat >> "$OUTPUT" <<'EOF'
<h3 class="section-title">Process Resources</h3>
<div class="grid">

  <div class="card">
    <h2>Memory RSS (Median) - lower is better</h2>
    <div class="insight" id="memInsight"></div>
    <canvas id="memChart"></canvas>
  </div>

  <div class="card">
    <h2>CPU Usage (Median) - lower is better</h2>
    <div class="insight" id="cpuInsight"></div>
    <canvas id="cpuChart"></canvas>
  </div>

  <div class="card">
    <h2>Peak RSS per Run (MB) - lower &amp; tighter is better</h2>
    <div class="insight" id="peakRssRunInsight"></div>
    <canvas id="peakRssRunChart"></canvas>
  </div>

  <div class="card">
    <h2>Avg CPU per Run (%) - lower &amp; tighter is better</h2>
    <div class="insight" id="avgCpuRunInsight"></div>
    <canvas id="avgCpuRunChart"></canvas>
  </div>

</div>

<h3 class="section-title">Correlation (API vs Resources)</h3>
<div class="grid">

  <div class="card">
    <h2>Throughput vs Memory per Run <span class="learn-more"><a href="#glossary-mem-per-rps">glossary &#x2193;</a></span></h2>
    <p style="font-size:0.75em; color:#8b949e; margin-bottom:8px;">
      RPS and Peak RSS overlaid per run. Higher RPS with lower memory = more efficient.
    </p>
    <div class="insight" id="rpsMemInsight"></div>
    <canvas id="rpsMemChart"></canvas>
  </div>

  <div class="card">
    <h2>Throughput vs CPU per Run <span class="learn-more"><a href="#glossary-cpu-per-rps">glossary &#x2193;</a></span></h2>
    <p style="font-size:0.75em; color:#8b949e; margin-bottom:8px;">
      RPS and Avg CPU overlaid per run. Higher RPS with lower CPU = more efficient.
    </p>
    <div class="insight" id="rpsCpuInsight"></div>
    <canvas id="rpsCpuChart"></canvas>
  </div>

  <div class="card">
    <h2>Latency vs Memory per Run</h2>
    <p style="font-size:0.75em; color:#8b949e; margin-bottom:8px;">
      P99 latency and Peak RSS overlaid per run. Lower latency with lower memory = more efficient.
    </p>
    <div class="insight" id="latMemInsight"></div>
    <canvas id="latMemChart"></canvas>
  </div>

  </div>

EOF
fi

cat >> "$OUTPUT" <<'STATICEOF'

<div class="variance-note">
  <h3>How to read this report</h3>
  <p>
    Each benchmark was run multiple times. The <strong>median</strong> is used for comparison because it
    ignores outliers (e.g., a single slow run caused by GC, OS scheduling, or network jitter).
  </p>
  <p style="margin-top:8px">
    If the <strong>average is significantly lower than the median</strong>, it means some runs were much
    slower than typical — this indicates <strong>high variance</strong>. High variance is itself a performance
    signal: it may indicate garbage collection pauses (common in Node.js/TypeScript runtimes like Sidecar),
    memory pressure, or unstable response times under load. A tool with lower variance delivers more
    predictable, reliable performance.
  </p>
  <p style="margin-top:8px">
    <strong>stdev</strong> (standard deviation) quantifies the spread. Lower stdev = more consistent runs.
  </p>
</div>
STATICEOF

cat >> "$OUTPUT" <<EOF
<script>
const LA = '${LABEL_A}';
const LB = '${LABEL_B}';
const colorA = 'rgba(56, 166, 247, 0.8)';
const colorB = 'rgba(240, 146, 53, 0.8)';
const colorALight = 'rgba(56, 166, 247, 0.15)';
const colorBLight = 'rgba(240, 146, 53, 0.15)';
const borderA = 'rgb(56, 166, 247)';
const borderB = 'rgb(240, 146, 53)';

Chart.defaults.color = '#c9d1d9';
Chart.defaults.borderColor = '#30363d';

// --- Helpers ---
function pctDiff(a, b) {
  if (a === 0) return 'N/A';
  return ((b - a) / a * 100).toFixed(1) + '%';
}
function winner(aVal, bVal, lowerIsBetter) {
  if (Math.abs(aVal - bVal) / Math.max(aVal, bVal) < 0.02) return 'neutral';
  if (lowerIsBetter) return aVal < bVal ? 'a' : 'b';
  return aVal > bVal ? 'a' : 'b';
}
function winnerText(w, metricName, aVal, bVal, lowerIsBetter) {
  const diff = Math.abs(((bVal - aVal) / aVal) * 100).toFixed(1);
  if (w === 'neutral') return '<span class="neutral">Virtually identical</span> — within 2% difference.';
  const winLabel = w === 'a' ? LA : LB;
  const betterWord = lowerIsBetter ? 'lower' : 'higher';
  return '<span class="better">' + winLabel + ' wins</span> with ' + diff + '% ' + betterWord + ' ' + metricName + '.';
}

// --- Median latency values ---
const mA = { avg:${M_A_AVG}, p50:${M_A_P50}, p75:${M_A_P75}, p90:${M_A_P90}, p95:${M_A_P95}, p99:${M_A_P99}, p999:${M_A_P999} };
const mB = { avg:${M_B_AVG}, p50:${M_B_P50}, p75:${M_B_P75}, p90:${M_B_P90}, p95:${M_B_P95}, p99:${M_B_P99}, p999:${M_B_P999} };
const mA_rps = ${M_A_RPS}, mB_rps = ${M_B_RPS};
const avgA_rps = ${V_A_RPS}, avgB_rps = ${V_B_RPS};
const sdA_rps = ${SD_A_RPS}, sdB_rps = ${SD_B_RPS};
const sdA_p99 = ${SD_A_P99}, sdB_p99 = ${SD_B_P99};

// Individual runs
const runsA_rps = [${ALL_RPS_A}];
const runsB_rps = [${ALL_RPS_B}];
const runsA_avg = [${ALL_AVG_A}];
const runsB_avg = [${ALL_AVG_B}];
const runsA_p99 = [${ALL_P99_A}];
const runsB_p99 = [${ALL_P99_B}];

// --- 1. Latency Percentiles Bar Chart ---
new Chart(document.getElementById('latencyChart'), {
  type: 'bar',
  data: {
    labels: ['Avg', 'P50', 'P75', 'P90', 'P95', 'P99', 'P99.9'],
    datasets: [
      { label: LA, data: [mA.avg, mA.p50, mA.p75, mA.p90, mA.p95, mA.p99, mA.p999],
        backgroundColor: colorA, borderColor: borderA, borderWidth: 1 },
      { label: LB, data: [mB.avg, mB.p50, mB.p75, mB.p90, mB.p95, mB.p99, mB.p999],
        backgroundColor: colorB, borderColor: borderB, borderWidth: 1 }
    ]
  },
  options: {
    responsive: true,
    plugins: { tooltip: { callbacks: { label: c => c.dataset.label + ': ' + c.parsed.y.toFixed(2) + 'ms' }}},
    scales: { y: { beginAtZero: true, title: { display: true, text: 'ms' }}}
  }
});
// Insight
(function(){
  const w = winner(mA.p99, mB.p99, true);
  let txt = winnerText(w, 'P99 latency', mA.p99, mB.p99, true);
  const tailA = mA.p99 / mA.p50, tailB = mB.p99 / mB.p50;
  if (tailA > tailB * 1.2) txt += ' ' + LA + ' has a longer tail (P99/P50 ratio: ' + tailA.toFixed(1) + 'x vs ' + tailB.toFixed(1) + 'x), meaning worse worst-case behavior.';
  else if (tailB > tailA * 1.2) txt += ' ' + LB + ' has a longer tail (P99/P50 ratio: ' + tailB.toFixed(1) + 'x vs ' + tailA.toFixed(1) + 'x), meaning worse worst-case behavior.';
  document.getElementById('latencyInsight').innerHTML = txt;
})();

// --- 2. RPS Bar (median + avg) ---
new Chart(document.getElementById('rpsChart'), {
  type: 'bar',
  data: {
    labels: ['Median', 'Average'],
    datasets: [
      { label: LA, data: [mA_rps, avgA_rps], backgroundColor: colorA, borderColor: borderA, borderWidth: 1 },
      { label: LB, data: [mB_rps, avgB_rps], backgroundColor: colorB, borderColor: borderB, borderWidth: 1 }
    ]
  },
  options: {
    responsive: true,
    plugins: { tooltip: { callbacks: { label: c => c.dataset.label + ': ' + c.parsed.y.toFixed(2) + ' RPS' }}},
    scales: { y: { beginAtZero: true, title: { display: true, text: 'Requests/sec' }}}
  }
});
(function(){
  const w = winner(mA_rps, mB_rps, false);
  let txt = winnerText(w, 'throughput', mA_rps, mB_rps, false);
  // Variance check
  const varA = avgA_rps < mA_rps * 0.95;
  const varB = avgB_rps < mB_rps * 0.95;
  if (varA && !varB) txt += ' <span class="worse">' + LA + ' shows high variance</span> — average is notably below median, indicating inconsistent runs.';
  if (varB && !varA) txt += ' <span class="worse">' + LB + ' shows high variance</span> — average is notably below median, indicating inconsistent runs (possible GC pauses or resource contention).';
  if (varA && varB) txt += ' Both tools show high variance (average well below median).';
  document.getElementById('rpsInsight').innerHTML = txt;
})();

// --- 3. Latency Distribution Line ---
new Chart(document.getElementById('distChart'), {
  type: 'line',
  data: {
    labels: ['P50', 'P75', 'P90', 'P95', 'P99', 'P99.9'],
    datasets: [
      { label: LA, data: [mA.p50, mA.p75, mA.p90, mA.p95, mA.p99, mA.p999],
        borderColor: borderA, backgroundColor: colorALight, fill: true, tension: 0.3, pointRadius: 5 },
      { label: LB, data: [mB.p50, mB.p75, mB.p90, mB.p95, mB.p99, mB.p999],
        borderColor: borderB, backgroundColor: colorBLight, fill: true, tension: 0.3, pointRadius: 5 }
    ]
  },
  options: {
    responsive: true,
    plugins: { tooltip: { callbacks: { label: c => c.dataset.label + ': ' + c.parsed.y.toFixed(2) + 'ms' }}},
    scales: { y: { beginAtZero: true, title: { display: true, text: 'ms' }}}
  }
});
(function(){
  const slopeA = mA.p999 - mA.p50, slopeB = mB.p999 - mB.p50;
  const w = winner(slopeA, slopeB, true);
  let txt = 'Spread from P50 to P99.9: ' + LA + '=' + slopeA.toFixed(1) + 'ms, ' + LB + '=' + slopeB.toFixed(1) + 'ms. ';
  if (w === 'a') txt += '<span class="better">' + LA + '</span> has a flatter curve — more predictable latency under load.';
  else if (w === 'b') txt += '<span class="better">' + LB + '</span> has a flatter curve — more predictable latency under load.';
  else txt += 'Both tools have similar latency spread.';
  document.getElementById('distInsight').innerHTML = txt;
})();

// --- 4. P99 per run scatter ---
const maxRuns = Math.max(runsA_p99.length, runsB_p99.length);
const runLabels = Array.from({length: maxRuns}, (_, i) => 'Run ' + (i+1));
new Chart(document.getElementById('p99Chart'), {
  type: 'bar',
  data: {
    labels: runLabels,
    datasets: [
      { label: LA, data: runsA_p99, backgroundColor: colorA, borderColor: borderA, borderWidth: 1 },
      { label: LB, data: runsB_p99, backgroundColor: colorB, borderColor: borderB, borderWidth: 1 }
    ]
  },
  options: {
    responsive: true,
    plugins: { tooltip: { callbacks: { label: c => c.dataset.label + ': ' + c.parsed.y.toFixed(2) + 'ms' }}},
    scales: { y: { beginAtZero: true, title: { display: true, text: 'P99 Latency (ms)' }}}
  }
});
(function(){
  const w = winner(mA.p99, mB.p99, true);
  let txt = winnerText(w, 'median P99', mA.p99, mB.p99, true);
  // Count outlier runs (>1.5x median) to give context
  function countOutliers(runs, med) {
    return runs.filter(v => v > med * 1.5 || v < med * 0.5).length;
  }
  const outA = countOutliers(runsA_p99, mA.p99), outB = countOutliers(runsB_p99, mB.p99);
  if (sdA_p99 > sdB_p99 * 1.5) {
    txt += ' <span class="worse">' + LA + ' has higher P99 variance</span> (stdev ' + sdA_p99.toFixed(1) + 'ms vs ' + sdB_p99.toFixed(1) + 'ms)';
    if (outA <= 1) txt += ' — driven by ' + outA + ' outlier run out of ' + runsA_p99.length + ', most runs are consistent.';
    else txt += ' — ' + outA + ' out of ' + runsA_p99.length + ' runs are outliers.';
  } else if (sdB_p99 > sdA_p99 * 1.5) {
    txt += ' <span class="worse">' + LB + ' has higher P99 variance</span> (stdev ' + sdB_p99.toFixed(1) + 'ms vs ' + sdA_p99.toFixed(1) + 'ms)';
    if (outB <= 1) txt += ' — driven by ' + outB + ' outlier run out of ' + runsB_p99.length + ', most runs are consistent.';
    else txt += ' — ' + outB + ' out of ' + runsB_p99.length + ' runs are outliers.';
  } else {
    txt += ' Both show consistent P99 across runs (stdev ' + sdA_p99.toFixed(1) + 'ms vs ' + sdB_p99.toFixed(1) + 'ms).';
  }
  document.getElementById('p99Insight').innerHTML = txt;
})();

// --- 5. RPS per run ---
new Chart(document.getElementById('rpsRunChart'), {
  type: 'bar',
  data: {
    labels: runLabels,
    datasets: [
      { label: LA, data: runsA_rps, backgroundColor: colorA, borderColor: borderA, borderWidth: 1 },
      { label: LB, data: runsB_rps, backgroundColor: colorB, borderColor: borderB, borderWidth: 1 }
    ]
  },
  options: {
    responsive: true,
    plugins: { tooltip: { callbacks: { label: c => c.dataset.label + ': ' + c.parsed.y.toFixed(2) + ' RPS' }}},
    scales: { y: { beginAtZero: true, title: { display: true, text: 'Requests/sec' }}}
  }
});
(function(){
  let txt = 'RPS stdev: ' + LA + '=' + sdA_rps.toFixed(2) + ', ' + LB + '=' + sdB_rps.toFixed(2) + '. ';
  const cvA = sdA_rps / mA_rps, cvB = sdB_rps / mB_rps;
  if (cvA > 0.1 || cvB > 0.1) {
    const unstable = cvA > cvB ? LA : LB;
    txt += '<span class="worse">' + unstable + ' has higher relative variance</span> (coefficient of variation: '
        + (cvA*100).toFixed(1) + '% vs ' + (cvB*100).toFixed(1) + '%). ';
    txt += 'High RPS variance suggests the tool\'s throughput is less stable — possible causes include garbage collection, connection pool churn, or runtime overhead.';
  } else {
    txt += 'Both tools show consistent throughput across runs.';
  }
  document.getElementById('rpsRunInsight').innerHTML = txt;
})();

// --- Request Duration ---
const totalReqsA = ${M_A_REQS}, totalReqsB = ${M_B_REQS};
const durA = { min:${M_A_MINLAT}, avg:${M_A_AVG}, stdev:${M_A_STDLAT}, max:${M_A_MAXLAT} };
const durB = { min:${M_B_MINLAT}, avg:${M_B_AVG}, stdev:${M_B_STDLAT}, max:${M_B_MAXLAT} };

new Chart(document.getElementById('durationChart'), {
  type: 'bar',
  data: {
    labels: ['Min', 'Avg', 'Stdev'],
    datasets: [
      { label: LA, data: [durA.min, durA.avg, durA.stdev],
        backgroundColor: colorA, borderColor: borderA, borderWidth: 1 },
      { label: LB, data: [durB.min, durB.avg, durB.stdev],
        backgroundColor: colorB, borderColor: borderB, borderWidth: 1 }
    ]
  },
  options: {
    responsive: true,
    plugins: { tooltip: { callbacks: { label: c => c.dataset.label + ': ' + c.parsed.y.toFixed(2) + 'ms' }}},
    scales: { y: { beginAtZero: true, title: { display: true, text: 'ms' }}}
  }
});
(function(){
  const w = winner(durA.avg, durB.avg, true);
  let txt = winnerText(w, 'avg request duration', durA.avg, durB.avg, true);
  // Compare consistency using coefficient of variation (CV = stdev/avg) — normalizes for different avg latencies
  const cvA = durA.avg > 0 ? durA.stdev / durA.avg : 0;
  const cvB = durB.avg > 0 ? durB.stdev / durB.avg : 0;
  if (cvA > 0 || cvB > 0) {
    const cvW = winner(cvA, cvB, true);
    if (cvW === 'a') {
      txt += ' <span class="better">' + LA + ' is also more consistent</span> relative to its avg (CV ' + (cvA*100).toFixed(1) + '% vs ' + (cvB*100).toFixed(1) + '%, stdev ' + durA.stdev.toFixed(1) + 'ms vs ' + durB.stdev.toFixed(1) + 'ms).';
    } else if (cvW === 'b') {
      txt += ' <span class="better">' + LB + ' is more consistent</span> relative to its avg (CV ' + (cvB*100).toFixed(1) + '% vs ' + (cvA*100).toFixed(1) + '%, stdev ' + durB.stdev.toFixed(1) + 'ms vs ' + durA.stdev.toFixed(1) + 'ms).';
    } else {
      txt += ' Both have similar consistency (CV ' + (cvA*100).toFixed(1) + '% vs ' + (cvB*100).toFixed(1) + '%).';
    }
  }
  txt += ' <span class="learn-more"><a href="#glossary-stdev">What does stdev mean? &#x2193;</a> | <a href="#glossary-cv">What does CV mean? &#x2193;</a></span>';
  document.getElementById('durationInsight').innerHTML = txt;
})();

// --- Max Request Duration ---
new Chart(document.getElementById('maxDurationChart'), {
  type: 'bar',
  data: {
    labels: [LA, LB],
    datasets: [
      { label: 'Max latency (ms)', data: [durA.max, durB.max],
        backgroundColor: [colorA, colorB], borderColor: [borderA, borderB], borderWidth: 1 }
    ]
  },
  options: {
    responsive: true,
    plugins: { legend: { display: false },
      tooltip: { callbacks: { label: c => c.parsed.y.toFixed(0) + 'ms' }}},
    scales: { y: { beginAtZero: true, title: { display: true, text: 'ms' }}}
  }
});
(function(){
  let txt = LA + ': ' + durA.max.toFixed(0) + 'ms | ' + LB + ': ' + durB.max.toFixed(0) + 'ms. ';
  txt += 'This is the <strong>single slowest request</strong> out of ~' + Math.max(totalReqsA, totalReqsB).toLocaleString() + ' total. ';
  txt += 'P99.9 is a more meaningful worst-case metric. <span class="learn-more"><a href="#glossary-max">Why is max so high? &#x2193;</a></span>';
  document.getElementById('maxDurationInsight').innerHTML = txt;
})();

// --- Response Size ---
const transferA = ${M_A_TRANSFER}, transferB = ${M_B_TRANSFER};
const bytesA = ${M_A_BYTES}, bytesB = ${M_B_BYTES};
const avgRespA = totalReqsA > 0 ? bytesA / totalReqsA : 0;
const avgRespB = totalReqsB > 0 ? bytesB / totalReqsB : 0;

function fmtBytes(b) {
  if (b >= 1e9) return (b/1e9).toFixed(2) + ' GB';
  if (b >= 1e6) return (b/1e6).toFixed(2) + ' MB';
  if (b >= 1e3) return (b/1e3).toFixed(2) + ' KB';
  return b.toFixed(0) + ' B';
}

// --- Transfer Rate chart ---
new Chart(document.getElementById('transferChart'), {
  type: 'bar',
  data: {
    labels: ['Transfer/sec'],
    datasets: [
      { label: LA, data: [transferA / 1e6],
        backgroundColor: colorA, borderColor: borderA, borderWidth: 1 },
      { label: LB, data: [transferB / 1e6],
        backgroundColor: colorB, borderColor: borderB, borderWidth: 1 }
    ]
  },
  options: {
    responsive: true,
    plugins: { tooltip: { callbacks: {
      label: function(c) { return c.dataset.label + ': ' + c.parsed.y.toFixed(2) + ' MB/s'; }
    }}},
    scales: { y: { beginAtZero: true, title: { display: true, text: 'MB/s' }}}
  }
});
(function(){
  const w = winner(transferA, transferB, false);
  let txt = LA + ': ' + fmtBytes(transferA) + '/s, ' + LB + ': ' + fmtBytes(transferB) + '/s. ';
  if (w === 'a') {
    const pct = ((transferA - transferB) / transferB * 100).toFixed(1);
    txt += '<span class="better"><strong>' + LA + '</strong> transfers ' + pct + '% more data per second.</span>';
  } else if (w === 'b') {
    const pct = ((transferB - transferA) / transferA * 100).toFixed(1);
    txt += '<span class="better"><strong>' + LB + '</strong> transfers ' + pct + '% more data per second.</span>';
  } else {
    txt += 'Transfer rates are within 2% — essentially equal.';
  }
  document.getElementById('transferInsight').innerHTML = txt;
})();

// --- Avg Response Size chart ---
new Chart(document.getElementById('respSizeChart'), {
  type: 'bar',
  data: {
    labels: ['Avg Response Size'],
    datasets: [
      { label: LA, data: [avgRespA / 1e3],
        backgroundColor: colorA, borderColor: borderA, borderWidth: 1 },
      { label: LB, data: [avgRespB / 1e3],
        backgroundColor: colorB, borderColor: borderB, borderWidth: 1 }
    ]
  },
  options: {
    responsive: true,
    plugins: { tooltip: { callbacks: {
      label: function(c) { return c.dataset.label + ': ' + c.parsed.y.toFixed(2) + ' KB'; }
    }}},
    scales: { y: { beginAtZero: true, title: { display: true, text: 'KB' }}}
  }
});
(function(){
  let txt = LA + ': ' + fmtBytes(avgRespA) + ', ' + LB + ': ' + fmtBytes(avgRespB) + '. ';
  const respDiff = Math.abs(avgRespA - avgRespB) / Math.max(avgRespA, avgRespB);
  if (respDiff > 0.1) {
    const bigger = avgRespA > avgRespB ? LA : LB;
    const pct = (respDiff * 100).toFixed(1);
    txt += '<span class="neutral"><strong>' + bigger + '</strong> returns ' + pct + '% larger responses</span> — throughput comparison should account for payload size difference.';
  } else {
    txt += 'Response sizes are similar — throughput comparison is apples-to-apples.';
  }
  document.getElementById('respSizeInsight').innerHTML = txt;
})();

// --- Error Rate per Run ---
const runsA_errors = [${ALL_ERRORS_A}];
const runsB_errors = [${ALL_ERRORS_B}];
const runsA_totalReqs = [${ALL_TOTAL_REQS_A}];
const runsB_totalReqs = [${ALL_TOTAL_REQS_B}];
const runsA_errorPct = runsA_errors.map((e, i) => runsA_totalReqs[i] > 0 ? (e / runsA_totalReqs[i]) * 100 : 0);
const runsB_errorPct = runsB_errors.map((e, i) => runsB_totalReqs[i] > 0 ? (e / runsB_totalReqs[i]) * 100 : 0);

new Chart(document.getElementById('errorChart'), {
  type: 'bar',
  data: {
    labels: runLabels,
    datasets: [
      { label: LA + ' errors', data: runsA_errors, backgroundColor: colorA, borderColor: borderA, borderWidth: 1 },
      { label: LB + ' errors', data: runsB_errors, backgroundColor: colorB, borderColor: borderB, borderWidth: 1 }
    ]
  },
  options: {
    responsive: true,
    plugins: { tooltip: { callbacks: {
      label: function(c) {
        const pct = c.datasetIndex === 0 ? runsA_errorPct[c.dataIndex] : runsB_errorPct[c.dataIndex];
        return c.dataset.label + ': ' + c.parsed.y + ' (' + pct.toFixed(2) + '%)';
      }
    }}},
    scales: { y: { beginAtZero: true, title: { display: true, text: 'Total Errors' }}}
  }
});
(function(){
  const totalA = runsA_errors.reduce((a,b)=>a+b,0);
  const totalB = runsB_errors.reduce((a,b)=>a+b,0);
  const reqsA = runsA_totalReqs.reduce((a,b)=>a+b,0);
  const reqsB = runsB_totalReqs.reduce((a,b)=>a+b,0);
  const pctA = reqsA > 0 ? (totalA / reqsA * 100).toFixed(3) : '0.000';
  const pctB = reqsB > 0 ? (totalB / reqsB * 100).toFixed(3) : '0.000';
  let txt;
  if (totalA === 0 && totalB === 0) {
    txt = '<span class="better">Zero errors</span> across all runs for both tools.';
  } else if (totalA === 0) {
    txt = '<span class="better">' + LA + ' had zero errors</span>. ' + LB + ' had ' + totalB + ' errors (' + pctB + '% of requests).';
  } else if (totalB === 0) {
    txt = '<span class="better">' + LB + ' had zero errors</span>. ' + LA + ' had ' + totalA + ' errors (' + pctA + '% of requests).';
  } else {
    txt = LA + ': ' + totalA + ' errors (' + pctA + '%). ' + LB + ': ' + totalB + ' errors (' + pctB + '%). ';
    const w = winner(totalA, totalB, true);
    if (w !== 'neutral') {
      const wl = w === 'a' ? LA : LB;
      txt += '<span class="better">' + wl + ' has fewer errors.</span>';
    }
  }
  document.getElementById('errorInsight').innerHTML = txt;
})();
EOF

if [ -n "$HAS_RESOURCES" ]; then
cat >> "$OUTPUT" <<EOF

// --- 6. Memory RSS Bar Chart ---
const memA = { start:${M_A_START_RSS}, peak:${M_A_PEAK_RSS}, end:${M_A_END_RSS}, delta:${M_A_DELTA_RSS} };
const memB = { start:${M_B_START_RSS}, peak:${M_B_PEAK_RSS}, end:${M_B_END_RSS}, delta:${M_B_DELTA_RSS} };

new Chart(document.getElementById('memChart'), {
  type: 'bar',
  data: {
    labels: ['Start RSS', 'Peak RSS', 'End RSS', 'Delta RSS'],
    datasets: [
      { label: LA, data: [memA.start, memA.peak, memA.end, memA.delta],
        backgroundColor: colorA, borderColor: borderA, borderWidth: 1 },
      { label: LB, data: [memB.start, memB.peak, memB.end, memB.delta],
        backgroundColor: colorB, borderColor: borderB, borderWidth: 1 }
    ]
  },
  options: {
    responsive: true,
    plugins: { tooltip: { callbacks: { label: c => c.dataset.label + ': ' + c.parsed.y.toFixed(2) + ' MB' }}},
    scales: { y: { beginAtZero: true, title: { display: true, text: 'MB' }}}
  }
});
(function(){
  const w = winner(memA.peak, memB.peak, true);
  let txt = winnerText(w, 'peak memory', memA.peak, memB.peak, true);
  if (memA.delta > memB.delta * 1.5) txt += ' <span class="worse">' + LA + ' grew ' + memA.delta.toFixed(1) + ' MB vs ' + memB.delta.toFixed(1) + ' MB</span> — higher memory growth under load.';
  else if (memB.delta > memA.delta * 1.5) txt += ' <span class="worse">' + LB + ' grew ' + memB.delta.toFixed(1) + ' MB vs ' + memA.delta.toFixed(1) + ' MB</span> — higher memory growth under load.';
  document.getElementById('memInsight').innerHTML = txt;
})();

// --- 7. CPU Bar Chart ---
const cpuA = { avg:${M_A_AVG_CPU}, peak:${M_A_PEAK_CPU} };
const cpuB = { avg:${M_B_AVG_CPU}, peak:${M_B_PEAK_CPU} };

new Chart(document.getElementById('cpuChart'), {
  type: 'bar',
  data: {
    labels: ['Avg CPU %', 'Peak CPU %'],
    datasets: [
      { label: LA, data: [cpuA.avg, cpuA.peak],
        backgroundColor: colorA, borderColor: borderA, borderWidth: 1 },
      { label: LB, data: [cpuB.avg, cpuB.peak],
        backgroundColor: colorB, borderColor: borderB, borderWidth: 1 }
    ]
  },
  options: {
    responsive: true,
    plugins: { tooltip: { callbacks: { label: c => c.dataset.label + ': ' + c.parsed.y.toFixed(1) + '%' }}},
    scales: { y: { beginAtZero: true, title: { display: true, text: 'CPU %' }}}
  }
});
(function(){
  const w = winner(cpuA.avg, cpuB.avg, true);
  let txt = winnerText(w, 'CPU usage', cpuA.avg, cpuB.avg, true);
  if (cpuA.peak > cpuB.peak * 1.3) txt += ' ' + LA + ' also has higher peak CPU (' + cpuA.peak.toFixed(1) + '% vs ' + cpuB.peak.toFixed(1) + '%).';
  else if (cpuB.peak > cpuA.peak * 1.3) txt += ' ' + LB + ' also has higher peak CPU (' + cpuB.peak.toFixed(1) + '% vs ' + cpuA.peak.toFixed(1) + '%).';
  // If the higher-CPU service also has higher throughput, note that raw CPU comparison is misleading
  const hiCpu = cpuA.avg > cpuB.avg ? 'a' : 'b';
  const hiRps = mA_rps > mB_rps ? 'a' : 'b';
  if (w !== 'neutral' && hiCpu === hiRps) {
    const hiLabel = hiCpu === 'a' ? LA : LB;
    txt += ' <em>Note: ' + hiLabel + ' also has higher throughput — see Correlation section for CPU per request.</em>';
  }
  document.getElementById('cpuInsight').innerHTML = txt;
})();

// --- 8. Peak RSS per run ---
const runsA_peakRss = [${ALL_PEAK_RSS_A}];
const runsB_peakRss = [${ALL_PEAK_RSS_B}];

new Chart(document.getElementById('peakRssRunChart'), {
  type: 'bar',
  data: {
    labels: runLabels,
    datasets: [
      { label: LA, data: runsA_peakRss, backgroundColor: colorA, borderColor: borderA, borderWidth: 1 },
      { label: LB, data: runsB_peakRss, backgroundColor: colorB, borderColor: borderB, borderWidth: 1 }
    ]
  },
  options: {
    responsive: true,
    plugins: { tooltip: { callbacks: { label: c => c.dataset.label + ': ' + c.parsed.y.toFixed(2) + ' MB' }}},
    scales: { y: { beginAtZero: true, title: { display: true, text: 'Peak RSS (MB)' }}}
  }
});
(function(){
  const avgA = runsA_peakRss.reduce((a,b)=>a+b,0)/runsA_peakRss.length;
  const avgB = runsB_peakRss.reduce((a,b)=>a+b,0)/runsB_peakRss.length;
  const w = winner(avgA, avgB, true);
  document.getElementById('peakRssRunInsight').innerHTML = winnerText(w, 'memory footprint', avgA, avgB, true);
})();

// --- 9. Avg CPU per run ---
const runsA_avgCpu = [${ALL_AVG_CPU_A}];
const runsB_avgCpu = [${ALL_AVG_CPU_B}];

new Chart(document.getElementById('avgCpuRunChart'), {
  type: 'bar',
  data: {
    labels: runLabels,
    datasets: [
      { label: LA, data: runsA_avgCpu, backgroundColor: colorA, borderColor: borderA, borderWidth: 1 },
      { label: LB, data: runsB_avgCpu, backgroundColor: colorB, borderColor: borderB, borderWidth: 1 }
    ]
  },
  options: {
    responsive: true,
    plugins: { tooltip: { callbacks: { label: c => c.dataset.label + ': ' + c.parsed.y.toFixed(1) + '%' }}},
    scales: { y: { beginAtZero: true, title: { display: true, text: 'Avg CPU (%)' }}}
  }
});
(function(){
  const avgA = runsA_avgCpu.reduce((a,b)=>a+b,0)/runsA_avgCpu.length;
  const avgB = runsB_avgCpu.reduce((a,b)=>a+b,0)/runsB_avgCpu.length;
  const w = winner(avgA, avgB, true);
  let txt;
  if (w !== 'neutral') {
    const wLabel = w === 'a' ? LA : LB;
    const pct = Math.abs(((avgB - avgA) / avgA) * 100).toFixed(1);
    txt = '<strong>' + wLabel + '</strong> uses <strong>' + pct + '%</strong> less CPU on average per run';
  } else {
    txt = 'Avg CPU per run is within 2% — essentially equal';
  }
  // If the higher-CPU service also has higher throughput, note that raw CPU comparison is misleading
  const hiCpu = avgA > avgB ? 'a' : 'b';
  const hiRps = mA_rps > mB_rps ? 'a' : 'b';
  if (w !== 'neutral' && hiCpu === hiRps) {
    const hiLabel = hiCpu === 'a' ? LA : LB;
    txt += ' <em>— but ' + hiLabel + ' also handles more requests. See Correlation section for CPU per request.</em>';
  }
  document.getElementById('avgCpuRunInsight').innerHTML = txt;
})();

// --- 10. Throughput vs Memory (dual-axis line) ---
new Chart(document.getElementById('rpsMemChart'), {
  type: 'line',
  data: {
    labels: runLabels,
    datasets: [
      { label: LA + ' RPS', data: runsA_rps, borderColor: borderA, backgroundColor: colorALight,
        fill: false, tension: 0.3, pointRadius: 4, yAxisID: 'yRps' },
      { label: LB + ' RPS', data: runsB_rps, borderColor: borderB, backgroundColor: colorBLight,
        fill: false, tension: 0.3, pointRadius: 4, yAxisID: 'yRps' },
      { label: LA + ' Peak RSS', data: runsA_peakRss, borderColor: borderA, backgroundColor: colorALight,
        fill: true, tension: 0.3, pointRadius: 4, borderDash: [6, 3], yAxisID: 'yMem' },
      { label: LB + ' Peak RSS', data: runsB_peakRss, borderColor: borderB, backgroundColor: colorBLight,
        fill: true, tension: 0.3, pointRadius: 4, borderDash: [6, 3], yAxisID: 'yMem' }
    ]
  },
  options: {
    responsive: true,
    interaction: { mode: 'index', intersect: false },
    plugins: { tooltip: { callbacks: {
      label: c => c.dataset.label + ': ' + c.parsed.y.toFixed(1) + (c.dataset.yAxisID === 'yRps' ? ' RPS' : ' MB')
    }}},
    scales: {
      yRps: { type: 'linear', position: 'left', beginAtZero: true,
              title: { display: true, text: 'Requests/sec' }},
      yMem: { type: 'linear', position: 'right', beginAtZero: true,
              title: { display: true, text: 'Peak RSS (MB)' },
              grid: { drawOnChartArea: false }}
    }
  }
});
(function(){
  const effA = mA_rps > 0 ? memA.peak / mA_rps * 1000 : 0;
  const effB = mB_rps > 0 ? memB.peak / mB_rps * 1000 : 0;
  const w = winner(effA, effB, true);
  const diff = Math.abs(((effB - effA) / effA) * 100).toFixed(1);
  const rpsDiff = Math.abs(((mA_rps - mB_rps) / Math.min(mA_rps, mB_rps)) * 100).toFixed(0);
  let txt = 'MB per 1K RPS: ' + LA + '=' + effA.toFixed(2) + ', ' + LB + '=' + effB.toFixed(2) + '. ';
  if (w === 'neutral') {
    txt += '<span class="neutral">Virtually identical</span> memory efficiency.';
  } else {
    const wl = w === 'a' ? LA : LB;
    const loser = w === 'a' ? LB : LA;
    const loserRps = w === 'a' ? mB_rps : mA_rps;
    const winnerRps = w === 'a' ? mA_rps : mB_rps;
    txt += '<span class="better">' + wl + ' uses ' + diff + '% less memory per 1K requests.</span>';
    if (winnerRps > loserRps) {
      txt += ' <span class="better">' + wl + ' also handles ' + rpsDiff + '% more requests.</span>';
    } else if (winnerRps < loserRps) {
      txt += ' <em>However, <span class="better">' + loser + ' handles ' + rpsDiff + '% more requests</span> — higher throughput requires more memory for buffers and connections.</em>';
    } else {
      txt += ' Both handle similar throughput.';
    }
  }
  txt += ' <span class="learn-more"><a href="#glossary-mem-per-rps">Why does this matter? &#x2193;</a></span>';
  document.getElementById('rpsMemInsight').innerHTML = txt;
})();

// --- 11. Throughput vs CPU (dual-axis line) ---
new Chart(document.getElementById('rpsCpuChart'), {
  type: 'line',
  data: {
    labels: runLabels,
    datasets: [
      { label: LA + ' RPS', data: runsA_rps, borderColor: borderA, backgroundColor: colorALight,
        fill: false, tension: 0.3, pointRadius: 4, yAxisID: 'yRps2' },
      { label: LB + ' RPS', data: runsB_rps, borderColor: borderB, backgroundColor: colorBLight,
        fill: false, tension: 0.3, pointRadius: 4, yAxisID: 'yRps2' },
      { label: LA + ' Avg CPU', data: runsA_avgCpu, borderColor: borderA, backgroundColor: colorALight,
        fill: true, tension: 0.3, pointRadius: 4, borderDash: [6, 3], yAxisID: 'yCpu' },
      { label: LB + ' Avg CPU', data: runsB_avgCpu, borderColor: borderB, backgroundColor: colorBLight,
        fill: true, tension: 0.3, pointRadius: 4, borderDash: [6, 3], yAxisID: 'yCpu' }
    ]
  },
  options: {
    responsive: true,
    interaction: { mode: 'index', intersect: false },
    plugins: { tooltip: { callbacks: {
      label: c => c.dataset.label + ': ' + c.parsed.y.toFixed(1) + (c.dataset.yAxisID === 'yRps2' ? ' RPS' : '%')
    }}},
    scales: {
      yRps2: { type: 'linear', position: 'left', beginAtZero: true,
               title: { display: true, text: 'Requests/sec' }},
      yCpu:  { type: 'linear', position: 'right', beginAtZero: true,
               title: { display: true, text: 'Avg CPU (%)' },
               grid: { drawOnChartArea: false }}
    }
  }
});
(function(){
  const effA = mA_rps > 0 ? cpuA.avg / mA_rps * 1000 : 0;
  const effB = mB_rps > 0 ? cpuB.avg / mB_rps * 1000 : 0;
  const w = winner(effA, effB, true);
  const diff = Math.abs(((effB - effA) / effA) * 100).toFixed(1);
  const rpsDiff = Math.abs(((mA_rps - mB_rps) / Math.min(mA_rps, mB_rps)) * 100).toFixed(0);
  let txt = 'CPU% per 1K RPS: ' + LA + '=' + effA.toFixed(2) + ', ' + LB + '=' + effB.toFixed(2) + '. ';
  if (w === 'neutral') {
    txt += '<span class="neutral">Virtually identical</span> CPU efficiency.';
  } else {
    const wl = w === 'a' ? LA : LB;
    const loser = w === 'a' ? LB : LA;
    const loserRps = w === 'a' ? mB_rps : mA_rps;
    const winnerRps = w === 'a' ? mA_rps : mB_rps;
    txt += '<span class="better">' + wl + ' uses ' + diff + '% less CPU per 1K requests.</span>';
    // Always show RPS context
    if (winnerRps > loserRps) {
      txt += ' <span class="better">' + wl + ' also handles ' + rpsDiff + '% more requests.</span>';
    } else if (winnerRps < loserRps) {
      txt += ' <em>However, <span class="better">' + loser + ' handles ' + rpsDiff + '% more requests</span> — it operates at a higher load point where CPU per request is naturally higher.</em>';
    } else {
      txt += ' Both handle similar throughput.';
    }
  }
  txt += ' <span class="learn-more"><a href="#glossary-cpu-per-rps">Why does this matter? &#x2193;</a></span>';
  document.getElementById('rpsCpuInsight').innerHTML = txt;
})();

// --- 12. Latency vs Memory (dual-axis line) ---
new Chart(document.getElementById('latMemChart'), {
  type: 'line',
  data: {
    labels: runLabels,
    datasets: [
      { label: LA + ' P99', data: runsA_p99, borderColor: borderA, backgroundColor: colorALight,
        fill: false, tension: 0.3, pointRadius: 4, yAxisID: 'yLat' },
      { label: LB + ' P99', data: runsB_p99, borderColor: borderB, backgroundColor: colorBLight,
        fill: false, tension: 0.3, pointRadius: 4, yAxisID: 'yLat' },
      { label: LA + ' Peak RSS', data: runsA_peakRss, borderColor: borderA, backgroundColor: colorALight,
        fill: true, tension: 0.3, pointRadius: 4, borderDash: [6, 3], yAxisID: 'yMem2' },
      { label: LB + ' Peak RSS', data: runsB_peakRss, borderColor: borderB, backgroundColor: colorBLight,
        fill: true, tension: 0.3, pointRadius: 4, borderDash: [6, 3], yAxisID: 'yMem2' }
    ]
  },
  options: {
    responsive: true,
    interaction: { mode: 'index', intersect: false },
    plugins: { tooltip: { callbacks: {
      label: c => c.dataset.label + ': ' + c.parsed.y.toFixed(1) + (c.dataset.yAxisID === 'yLat' ? ' ms' : ' MB')
    }}},
    scales: {
      yLat:  { type: 'linear', position: 'left', beginAtZero: true,
               title: { display: true, text: 'P99 Latency (ms)' }},
      yMem2: { type: 'linear', position: 'right', beginAtZero: true,
               title: { display: true, text: 'Peak RSS (MB)' },
               grid: { drawOnChartArea: false }}
    }
  }
});
(function(){
  // Memory cost per ms of P99 latency — lower means you get better latency for less memory
  const costA = mA.p99 > 0 ? memA.peak / mA.p99 : 0;
  const costB = mB.p99 > 0 ? memB.peak / mB.p99 : 0;
  let txt = 'MB per ms of P99: ' + LA + '=' + costA.toFixed(2) + ', ' + LB + '=' + costB.toFixed(2) + '. ';
  // The service with lower latency AND lower memory is the clear winner
  // Otherwise compare the tradeoff
  if (mA.p99 < mB.p99 && memA.peak <= memB.peak) {
    txt += '<span class="better">' + LA + ' wins both</span> — lower latency with same or less memory.';
  } else if (mB.p99 < mA.p99 && memB.peak <= memA.peak) {
    txt += '<span class="better">' + LB + ' wins both</span> — lower latency with same or less memory.';
  } else if (mA.p99 < mB.p99) {
    const latPct = Math.abs(((mB.p99 - mA.p99) / mA.p99) * 100).toFixed(1);
    const memPct = Math.abs(((memA.peak - memB.peak) / memB.peak) * 100).toFixed(1);
    txt += '<span class="neutral">' + LA + ' has ' + latPct + '% lower P99 latency but uses ' + memPct + '% more memory.</span>';
  } else if (mB.p99 < mA.p99) {
    const latPct = Math.abs(((mA.p99 - mB.p99) / mB.p99) * 100).toFixed(1);
    const memPct = Math.abs(((memB.peak - memA.peak) / memA.peak) * 100).toFixed(1);
    txt += '<span class="neutral">' + LB + ' has ' + latPct + '% lower P99 latency but uses ' + memPct + '% more memory.</span>';
  } else {
    txt += '<span class="neutral">Virtually identical</span> latency-memory tradeoff.';
  }
  document.getElementById('latMemInsight').innerHTML = txt;
})();
EOF
fi

cat >> "$OUTPUT" <<'GLOSSARYEOF'
</script>

<div class="glossary">
  <h3>Glossary</h3>

  <h4 id="glossary-stdev">Stdev (Standard Deviation) in Request Duration</h4>
  <p>
    Stdev measures how spread out the request latencies are. A <strong>low stdev</strong> means most requests
    complete in a similar time — the service is <strong>predictable</strong>. A <strong>high stdev</strong> means
    some requests are much slower than others — the service is <strong>inconsistent</strong>.
  </p>
  <p>
    A service can have a lower average latency but higher stdev. This means it's usually faster, but occasionally
    has slow outliers. Common causes: <strong>metadata validation</strong> on first requests (subxt re-validates
    storage paths against runtime metadata), <strong>connection pool warmup</strong> (new WebSocket connections
    are slower than reused ones), <strong>node-side cache misses</strong> (first access to a storage key hits disk),
    or <strong>thread scheduling</strong> (multi-threaded runtimes like Tokio have more jitter than single-threaded
    event loops like Node.js).
  </p>
  <p>
    The stdev is computed across <strong>all requests</strong> in the run (often tens of thousands).
    Look at P50 vs P99 to see the actual spread — if P99 is much higher than P50, the slow outliers
    are what's driving the stdev up.
  </p>
  <p class="back-link"><a href="#card-duration">&#x2191; Back to Request Duration chart</a></p>

  <h4 id="glossary-cv">Coefficient of Variation (CV)</h4>
  <p>
    CV = stdev / avg, expressed as a percentage. It measures how large the spread is
    <strong>relative to the average</strong>. This is more meaningful than raw stdev when comparing
    two services with different average latencies.
  </p>
  <p>
    <strong>Example</strong>: Service A has avg 50ms, stdev 200ms (CV = 400%). Service B has avg 500ms,
    stdev 265ms (CV = 53%). Raw stdev says A is "more consistent" (200 &lt; 265), but A's responses
    swing &plusmn;4x its average — that's wildly unpredictable. B's variation is only &plusmn;53% of its average —
    much more proportionally stable.
  </p>
  <p>
    <strong>Lower CV = more consistent.</strong> A CV under ~50% is typical for a stable service under load.
    A CV over 100% means the stdev exceeds the average — response times are highly variable.
  </p>
  <p class="back-link"><a href="#card-duration">&#x2191; Back to Request Duration chart</a></p>

  <h4 id="glossary-max">Why is Max Latency So High?</h4>
  <p>
    The max is the <strong>single slowest request</strong> out of the entire run — one request out of
    potentially hundreds of thousands. It is almost always an outlier caused by:
  </p>
  <p>
    &bull; <strong>Cold start effects</strong> — the very first request may trigger metadata fetching,
    connection establishment, or cache population<br>
    &bull; <strong>Node-side storage misses</strong> — a particular block or account that wasn't in
    the RPC node's memory cache, forcing a disk read<br>
    &bull; <strong>GC pauses</strong> — garbage collection in Node.js (sidecar) or memory allocation
    spikes in Rust (rest-api)<br>
    &bull; <strong>Network jitter</strong> — a brief WebSocket hiccup between the API and the node<br>
    &bull; <strong>OS scheduling</strong> — the process was briefly preempted by the kernel
  </p>
  <p>
    The max is <strong>not representative</strong> of user experience. Use <strong>P99</strong> (worst 1% of
    requests) or <strong>P99.9</strong> (worst 0.1%) for a realistic worst-case. To investigate what caused
    a specific slow request, you would need request-level logging with timing breakdowns (time in RPC call
    vs serialization vs network).
  </p>
  <p class="back-link"><a href="#card-max-duration">&#x2191; Back to Max Duration chart</a></p>

  <h4 id="glossary-cpu-per-rps">CPU per 1K Requests — Why Throughput Matters</h4>
  <p>
    Comparing raw CPU usage between two services is misleading if they handle different amounts of traffic.
    A service processing 2x more requests <strong>should</strong> use more CPU — that's expected.
  </p>
  <p>
    <strong>CPU% per 1K RPS</strong> normalizes by throughput: it divides average CPU usage by requests per second.
    Lower = more efficient. However, this metric has a subtle bias: at higher throughput, CPU per request can
    increase due to <strong>contention</strong> (lock contention, cache pressure, context switching). The
    lower-throughput service may appear "more efficient" simply because it never reached the load level
    where these effects kick in.
  </p>
  <p>
    To get a fair comparison, both services should ideally be tested at the <strong>same request rate</strong>
    (using wrk2's constant-rate mode) rather than at their respective maximums.
  </p>
  <p class="back-link"><a href="#card-duration">&#x2191; Back to charts</a></p>

  <h4 id="glossary-mem-per-rps">Memory per 1K Requests — Why Throughput Matters</h4>
  <p>
    Similar to CPU, comparing raw memory usage is misleading at different throughput levels. A service handling
    more concurrent requests needs more memory for <strong>connection buffers</strong>, <strong>in-flight
    request state</strong>, and <strong>response serialization buffers</strong>.
  </p>
  <p>
    <strong>MB per 1K RPS</strong> normalizes by throughput. A service that uses 200MB to serve 1000 RPS
    is more memory-efficient than one that uses 150MB to serve 500 RPS (0.2 MB/1K vs 0.3 MB/1K).
  </p>
  <p>
    Note: some memory usage is <strong>fixed overhead</strong> (runtime, metadata cache, connection pools)
    that doesn't scale with request rate. This fixed cost is amortized better at higher throughput,
    which can make higher-throughput services appear more memory-efficient even if their per-request
    allocation is similar.
  </p>
  <p class="back-link"><a href="#card-duration">&#x2191; Back to charts</a></p>
</div>

GLOSSARYEOF

# --- Raw Data Table ---
cat >> "$OUTPUT" <<'RAWTABLESTYLE'

<div class="section-title">Raw Data — All Runs</div>
<div class="card full" style="overflow-x: auto;">
  <h2>Per-Run Results</h2>
  <p class="insight">Every individual benchmark run with all metrics. Scroll right for resource data.</p>
  <table style="width: 100%; border-collapse: collapse; font-size: 0.8em; margin-top: 12px;">
    <thead>
      <tr style="border-bottom: 2px solid #30363d; text-align: right;">
        <th style="text-align: left; padding: 8px 6px;">Service</th>
        <th style="padding: 8px 6px;">Run</th>
        <th style="padding: 8px 6px;">RPS</th>
        <th style="padding: 8px 6px;">Avg (ms)</th>
        <th style="padding: 8px 6px;">P50 (ms)</th>
        <th style="padding: 8px 6px;">P75 (ms)</th>
        <th style="padding: 8px 6px;">P90 (ms)</th>
        <th style="padding: 8px 6px;">P95 (ms)</th>
        <th style="padding: 8px 6px;">P99 (ms)</th>
        <th style="padding: 8px 6px;">P999 (ms)</th>
        <th style="padding: 8px 6px;">Requests</th>
        <th style="padding: 8px 6px;">Errors</th>
        <th style="padding: 8px 6px;">Peak RSS</th>
        <th style="padding: 8px 6px;">Avg CPU</th>
        <th style="padding: 8px 6px;">Peak CPU</th>
      </tr>
    </thead>
    <tbody>
RAWTABLESTYLE

RUN_NUM=0
for f in "${FILES_A[@]}"; do
    RUN_NUM=$((RUN_NUM + 1))
    jq -r --arg svc "$LABEL_A" --argjson run "$RUN_NUM" '
        "<tr style=\"border-bottom: 1px solid #21262d; text-align: right;\">" +
        "<td style=\"text-align: left; padding: 6px; color: #3fb950; font-weight: 600;\">" + $svc + "</td>" +
        "<td style=\"padding: 6px;\">" + ($run | tostring) + "</td>" +
        "<td style=\"padding: 6px;\">" + ((.rps // 0) * 10 | round / 10 | tostring) + "</td>" +
        "<td style=\"padding: 6px;\">" + ((.avg_latency_ms // 0) * 10 | round / 10 | tostring) + "</td>" +
        "<td style=\"padding: 6px;\">" + ((.p50_ms // 0) * 10 | round / 10 | tostring) + "</td>" +
        "<td style=\"padding: 6px;\">" + ((.p75_ms // 0) * 10 | round / 10 | tostring) + "</td>" +
        "<td style=\"padding: 6px;\">" + ((.p90_ms // 0) * 10 | round / 10 | tostring) + "</td>" +
        "<td style=\"padding: 6px;\">" + ((.p95_ms // 0) * 10 | round / 10 | tostring) + "</td>" +
        "<td style=\"padding: 6px;\">" + ((.p99_ms // 0) * 10 | round / 10 | tostring) + "</td>" +
        "<td style=\"padding: 6px;\">" + ((.p999_ms // 0) * 10 | round / 10 | tostring) + "</td>" +
        "<td style=\"padding: 6px;\">" + ((.total_requests // 0) | tostring) + "</td>" +
        "<td style=\"padding: 6px;\">" + (((.errors_total // .errors) // 0) | tostring) + "</td>" +
        "<td style=\"padding: 6px;\">" + (if .resources.peak_rss_mb then ((.resources.peak_rss_mb * 10 | round / 10 | tostring) + " MB") else "\u2014" end) + "</td>" +
        "<td style=\"padding: 6px;\">" + (if .resources.avg_cpu_pct then ((.resources.avg_cpu_pct * 10 | round / 10 | tostring) + "%") else "\u2014" end) + "</td>" +
        "<td style=\"padding: 6px;\">" + (if .resources.peak_cpu_pct then ((.resources.peak_cpu_pct * 10 | round / 10 | tostring) + "%") else "\u2014" end) + "</td>" +
        "</tr>"
    ' "$f" >> "$OUTPUT"
done

echo '<tr style="border-bottom: 2px solid #30363d;"><td colspan="15"></td></tr>' >> "$OUTPUT"

RUN_NUM=0
for f in "${FILES_B[@]}"; do
    RUN_NUM=$((RUN_NUM + 1))
    jq -r --arg svc "$LABEL_B" --argjson run "$RUN_NUM" '
        "<tr style=\"border-bottom: 1px solid #21262d; text-align: right;\">" +
        "<td style=\"text-align: left; padding: 6px; color: #58a6ff; font-weight: 600;\">" + $svc + "</td>" +
        "<td style=\"padding: 6px;\">" + ($run | tostring) + "</td>" +
        "<td style=\"padding: 6px;\">" + ((.rps // 0) * 10 | round / 10 | tostring) + "</td>" +
        "<td style=\"padding: 6px;\">" + ((.avg_latency_ms // 0) * 10 | round / 10 | tostring) + "</td>" +
        "<td style=\"padding: 6px;\">" + ((.p50_ms // 0) * 10 | round / 10 | tostring) + "</td>" +
        "<td style=\"padding: 6px;\">" + ((.p75_ms // 0) * 10 | round / 10 | tostring) + "</td>" +
        "<td style=\"padding: 6px;\">" + ((.p90_ms // 0) * 10 | round / 10 | tostring) + "</td>" +
        "<td style=\"padding: 6px;\">" + ((.p95_ms // 0) * 10 | round / 10 | tostring) + "</td>" +
        "<td style=\"padding: 6px;\">" + ((.p99_ms // 0) * 10 | round / 10 | tostring) + "</td>" +
        "<td style=\"padding: 6px;\">" + ((.p999_ms // 0) * 10 | round / 10 | tostring) + "</td>" +
        "<td style=\"padding: 6px;\">" + ((.total_requests // 0) | tostring) + "</td>" +
        "<td style=\"padding: 6px;\">" + (((.errors_total // .errors) // 0) | tostring) + "</td>" +
        "<td style=\"padding: 6px;\">" + (if .resources.peak_rss_mb then ((.resources.peak_rss_mb * 10 | round / 10 | tostring) + " MB") else "\u2014" end) + "</td>" +
        "<td style=\"padding: 6px;\">" + (if .resources.avg_cpu_pct then ((.resources.avg_cpu_pct * 10 | round / 10 | tostring) + "%") else "\u2014" end) + "</td>" +
        "<td style=\"padding: 6px;\">" + (if .resources.peak_cpu_pct then ((.resources.peak_cpu_pct * 10 | round / 10 | tostring) + "%") else "\u2014" end) + "</td>" +
        "</tr>"
    ' "$f" >> "$OUTPUT"
done

cat >> "$OUTPUT" <<'RAWTABLEEND'
    </tbody>
  </table>
</div>
RAWTABLEEND

cat >> "$OUTPUT" <<EOF

<p class="meta">
  Generated by compare_runs.sh | ${LABEL_A} (${#FILES_A[@]} runs) vs ${LABEL_B} (${#FILES_B[@]} runs) | $(date +%Y-%m-%d\ %H:%M:%S)
</p>

</body>
</html>
EOF

echo "Report saved: $OUTPUT"
echo "Open with: open $OUTPUT"
