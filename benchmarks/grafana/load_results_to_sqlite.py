#!/usr/bin/env python3
# Copyright (C) 2026 Parity Technologies (UK) Ltd.
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Load benchmark JSON results and resource CSV files into a SQLite database
# for visualization in Grafana (using the frser-sqlite-datasource plugin).
#
# Usage:
#   python3 load_results_to_sqlite.py <results_dir> [<results_dir> ...] [--db output.db]
#
# Examples:
#   python3 load_results_to_sqlite.py ../results/rest-api ../results/sidecar
#   python3 load_results_to_sqlite.py ~/results --db data/benchmarks.db

import argparse
import csv
import json
import os
import re
import sqlite3
import sys
from pathlib import Path

SCRIPT_DIR = Path(__file__).parent
DEFAULT_DB = SCRIPT_DIR / "data" / "benchmarks.db"

SCHEMA = """
CREATE TABLE IF NOT EXISTS benchmark_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    service TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    scenario TEXT NOT NULL,
    hardware TEXT NOT NULL,
    chain TEXT,
    run_number INTEGER,
    total_runs INTEGER,
    threads INTEGER,
    connections INTEGER,
    duration_s REAL,
    rps REAL,
    total_requests INTEGER,
    bytes INTEGER,
    transfer_per_sec REAL,
    avg_latency_ms REAL,
    stdev_ms REAL,
    min_latency_ms REAL,
    max_latency_ms REAL,
    p50_ms REAL,
    p75_ms REAL,
    p90_ms REAL,
    p95_ms REAL,
    p99_ms REAL,
    p999_ms REAL,
    req_sec_avg REAL,
    req_sec_stdev REAL,
    req_sec_max REAL,
    errors_total INTEGER DEFAULT 0,
    errors_connect INTEGER DEFAULT 0,
    errors_read INTEGER DEFAULT 0,
    errors_write INTEGER DEFAULT 0,
    errors_timeout INTEGER DEFAULT 0,
    errors_status INTEGER DEFAULT 0,
    start_rss_mb REAL,
    peak_rss_mb REAL,
    end_rss_mb REAL,
    delta_rss_mb REAL,
    avg_cpu_pct REAL,
    peak_cpu_pct REAL,
    resource_samples INTEGER,
    session_id TEXT,
    source_file TEXT,
    loaded_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS resource_samples (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    service TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    session_id TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    elapsed_s INTEGER NOT NULL,
    rss_kb INTEGER,
    vsz_kb INTEGER,
    rss_mb REAL,
    cpu_pct REAL,
    source_file TEXT,
    loaded_at TEXT DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_runs_service ON benchmark_runs(service);
CREATE INDEX IF NOT EXISTS idx_runs_endpoint ON benchmark_runs(endpoint);
CREATE INDEX IF NOT EXISTS idx_runs_scenario ON benchmark_runs(scenario);
CREATE INDEX IF NOT EXISTS idx_runs_service_endpoint ON benchmark_runs(service, endpoint);
CREATE INDEX IF NOT EXISTS idx_samples_session ON resource_samples(session_id);
CREATE INDEX IF NOT EXISTS idx_samples_service_endpoint ON resource_samples(service, endpoint);
"""


def load_json_file(cursor, json_path):
    """Load a single benchmark JSON result file."""
    with open(json_path) as f:
        d = json.load(f)

    # Skip summary files
    if "_summary_" in json_path.name:
        return False

    res = d.get("resources") or {}

    cursor.execute("""
        INSERT INTO benchmark_runs (
            service, endpoint, scenario, hardware, chain,
            run_number, total_runs, threads, connections, duration_s,
            rps, total_requests, bytes, transfer_per_sec,
            avg_latency_ms, stdev_ms, min_latency_ms, max_latency_ms,
            p50_ms, p75_ms, p90_ms, p95_ms, p99_ms, p999_ms,
            req_sec_avg, req_sec_stdev, req_sec_max,
            errors_total, errors_connect, errors_read, errors_write, errors_timeout, errors_status,
            start_rss_mb, peak_rss_mb, end_rss_mb, delta_rss_mb,
            avg_cpu_pct, peak_cpu_pct, resource_samples,
            session_id, source_file
        ) VALUES (
            ?, ?, ?, ?, ?,
            ?, ?, ?, ?, ?,
            ?, ?, ?, ?,
            ?, ?, ?, ?,
            ?, ?, ?, ?, ?, ?,
            ?, ?, ?,
            ?, ?, ?, ?, ?, ?,
            ?, ?, ?, ?,
            ?, ?, ?,
            ?, ?
        )
    """, (
        d.get("service", "unknown"),
        d.get("endpoint", "unknown"),
        d.get("scenario", "unknown"),
        d.get("hardware", "unknown"),
        d.get("chain"),
        d.get("run_number"),
        d.get("total_runs"),
        d.get("threads"),
        d.get("connections"),
        d.get("duration_s"),
        d.get("rps"),
        d.get("total_requests"),
        d.get("bytes"),
        d.get("transfer_per_sec"),
        d.get("avg_latency_ms"),
        d.get("stdev_ms"),
        d.get("min_latency_ms"),
        d.get("max_latency_ms"),
        d.get("p50_ms"),
        d.get("p75_ms"),
        d.get("p90_ms"),
        d.get("p95_ms"),
        d.get("p99_ms"),
        d.get("p999_ms"),
        d.get("req_sec_avg"),
        d.get("req_sec_stdev"),
        d.get("req_sec_max"),
        d.get("errors_total", d.get("errors", 0)),
        d.get("errors_connect", 0),
        d.get("errors_read", 0),
        d.get("errors_write", 0),
        d.get("errors_timeout", 0),
        d.get("errors_status", 0),
        res.get("start_rss_mb"),
        res.get("peak_rss_mb"),
        res.get("end_rss_mb"),
        res.get("delta_rss_mb"),
        res.get("avg_cpu_pct"),
        res.get("peak_cpu_pct"),
        res.get("samples"),
        res.get("session_id"),
        json_path.name,
    ))
    return True


def load_csv_file(cursor, csv_path):
    """Load a single resource CSV file."""
    with open(csv_path) as f:
        header_line = f.readline()
        # Skip the column header line
        f.readline()

        # Parse metadata from comment line
        service = "unknown"
        endpoint = "unknown"
        session_id = "unknown"

        m = re.search(r"service:\s*(\S+)", header_line)
        if m:
            service = m.group(1)
        m = re.search(r"endpoint:\s*(\S+)", header_line)
        if m:
            endpoint = m.group(1)
        m = re.search(r"run_id:\s*(\S+)", header_line)
        if m:
            session_id = m.group(1)

        rows = []
        reader = csv.reader(f)
        for row in reader:
            if len(row) < 6 or not row[0]:
                continue
            rows.append((
                service, endpoint, session_id,
                row[0],       # timestamp
                int(row[1]),  # elapsed_s
                int(row[2]),  # rss_kb
                int(row[3]),  # vsz_kb
                float(row[4]),  # rss_mb
                float(row[5]),  # cpu_pct
                csv_path.name,
            ))

        cursor.executemany("""
            INSERT INTO resource_samples
                (service, endpoint, session_id, timestamp, elapsed_s,
                 rss_kb, vsz_kb, rss_mb, cpu_pct, source_file)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        """, rows)

    return len(rows)


def load_directory(cursor, dir_path):
    """Load all JSON and CSV files from a directory tree."""
    dir_path = Path(dir_path)
    json_count = 0
    csv_count = 0
    sample_count = 0

    for json_file in sorted(dir_path.rglob("*.json")):
        if load_json_file(cursor, json_file):
            json_count += 1

    for csv_file in sorted(dir_path.rglob("*.csv")):
        n = load_csv_file(cursor, csv_file)
        csv_count += 1
        sample_count += n

    print(f"  Loaded {json_count} JSON files, {csv_count} CSV files ({sample_count} samples) from {dir_path}")


def print_summary(conn):
    """Print a summary of the loaded data."""
    print()
    print("=" * 60)

    print("\nBenchmark Runs:")
    print(f"  {'Service':<25} {'Endpoint':<15} {'Scenario':<15} {'Runs':>5} {'Avg RPS':>10} {'Avg Lat':>10} {'Avg P99':>10}")
    print(f"  {'-'*25} {'-'*15} {'-'*15} {'-'*5} {'-'*10} {'-'*10} {'-'*10}")
    for row in conn.execute("""
        SELECT service, endpoint, scenario, COUNT(*) as runs,
               ROUND(AVG(rps), 1), ROUND(AVG(avg_latency_ms), 1), ROUND(AVG(p99_ms), 1)
        FROM benchmark_runs
        GROUP BY service, endpoint, scenario
        ORDER BY service, endpoint
    """):
        print(f"  {row[0]:<25} {row[1]:<15} {row[2]:<15} {row[3]:>5} {row[4]:>10} {row[5]:>10} {row[6]:>10}")

    print("\nResource Samples:")
    print(f"  {'Service':<25} {'Endpoint':<15} {'Samples':>8} {'Min RSS':>10} {'Max RSS':>10} {'Max CPU':>10}")
    print(f"  {'-'*25} {'-'*15} {'-'*8} {'-'*10} {'-'*10} {'-'*10}")
    for row in conn.execute("""
        SELECT service, endpoint, COUNT(*), ROUND(MIN(rss_mb), 1),
               ROUND(MAX(rss_mb), 1), ROUND(MAX(cpu_pct), 1)
        FROM resource_samples
        GROUP BY service, endpoint
    """):
        print(f"  {row[0]:<25} {row[1]:<15} {row[2]:>8} {row[3]:>10} {row[4]:>10} {row[5]:>10}")

    print()
    print("=" * 60)


def main():
    parser = argparse.ArgumentParser(
        description="Load benchmark results into SQLite for Grafana visualization"
    )
    parser.add_argument("dirs", nargs="+", help="Result directories to load")
    parser.add_argument("--db", default=str(DEFAULT_DB), help="Output database path")
    args = parser.parse_args()

    db_path = Path(args.db)
    db_path.parent.mkdir(parents=True, exist_ok=True)

    # Remove existing DB to start fresh
    if db_path.exists():
        db_path.unlink()

    conn = sqlite3.connect(str(db_path))
    cursor = conn.cursor()
    cursor.executescript(SCHEMA)

    for d in args.dirs:
        if not Path(d).is_dir():
            print(f"Warning: {d} is not a directory, skipping")
            continue
        print(f"\nLoading: {d}")
        load_directory(cursor, d)

    conn.commit()
    print(f"\nDatabase: {db_path}")
    print_summary(conn)
    conn.close()

    print(f"\nDone. Run 'docker-compose up' in {SCRIPT_DIR} to start Grafana.")


if __name__ == "__main__":
    main()
