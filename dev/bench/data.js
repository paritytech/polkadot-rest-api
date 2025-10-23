window.BENCHMARK_DATA = {
  "lastUpdate": 1761250421385,
  "repoUrl": "https://github.com/paritytech/polkadot-rest-api",
  "entries": {
    "Benchmark": [
      {
        "commit": {
          "author": {
            "name": "paritytech",
            "username": "paritytech"
          },
          "committer": {
            "name": "paritytech",
            "username": "paritytech"
          },
          "id": "46805fe35e0879e7875c319283eeb7290130d338",
          "message": "feat: Benchmarking",
          "timestamp": "2025-10-23T15:16:21Z",
          "url": "https://github.com/paritytech/polkadot-rest-api/pull/14/commits/46805fe35e0879e7875c319283eeb7290130d338"
        },
        "date": 1761250419654,
        "tool": "customSmallerIsBetter",
        "benches": [
          {
            "name": "health - Avg Latency",
            "value": 1.06,
            "unit": "ms"
          },
          {
            "name": "health - P50 Latency",
            "value": 1.04,
            "unit": "ms"
          },
          {
            "name": "health - P90 Latency",
            "value": 1.33,
            "unit": "ms"
          },
          {
            "name": "health - P99 Latency",
            "value": 2.94,
            "unit": "ms"
          },
          {
            "name": "health - Throughput",
            "value": 44130.09,
            "unit": "req/sec",
            "extra": "biggerIsBetter"
          }
        ]
      }
    ]
  }
}