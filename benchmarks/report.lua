-- Report module for wrk benchmarks
-- Outputs JSON with latency percentiles and throughput to stderr
-- Used by run.sh to capture structured results and save to results/ directory
--
-- Usage: wrk automatically calls the done() function when the benchmark completes.
-- The JSON is written to stderr so it doesn't mix with wrk's stdout output.

local report = {}

function report.done()
    return function(summary, latency, requests)
        local endpoint = os.getenv("BENCH_ENDPOINT") or "unknown"
        local service = os.getenv("BENCH_SERVICE") or "polkadot-rest-api"

        local rps = summary.requests / (summary.duration / 1e6)
        local errors = summary.errors.status
            + summary.errors.connect
            + summary.errors.read
            + summary.errors.write
            + summary.errors.timeout

        local json = string.format(
            '{"endpoint":"%s","service":"%s","rps":%.2f,"avg_latency_ms":%.2f,"p50_ms":%.2f,"p90_ms":%.2f,"p95_ms":%.2f,"p99_ms":%.2f,"p999_ms":%.2f,"errors":%d,"total_requests":%d,"duration_s":%.2f}',
            endpoint,
            service,
            rps,
            latency.mean / 1000,
            latency:percentile(50) / 1000,
            latency:percentile(90) / 1000,
            latency:percentile(95) / 1000,
            latency:percentile(99) / 1000,
            latency:percentile(99.9) / 1000,
            errors,
            summary.requests,
            summary.duration / 1e6
        )

        -- Write JSON to stderr (captured by run.sh)
        io.stderr:write(json .. "\n")

        -- Print human-readable summary to stdout (matches original util.done() format)
        print("--------------------------")
        print("Total completed requests:       ", summary.requests)
        print("Failed requests:                ", summary.errors.status)
        print("Timeouts:                       ", summary.errors.connect or 0)
        print("Avg RequestTime(Latency):          "..string.format("%.2f", latency.mean / 1000).."ms")
        print("Max RequestTime(Latency):          "..(latency.max / 1000).."ms")
        print("Min RequestTime(Latency):          "..(latency.min / 1000).."ms")
        print("Benchmark finished.")
    end
end

return report
