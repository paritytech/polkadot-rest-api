# Adding Benchmarks for New Endpoints

Quick guide to add performance benchmarks for new API endpoints.

## 📋 Overview

Each endpoint benchmark consists of:
- **Directory**: `benchmarks/your_endpoint/`
- **Runner script**: `init.sh` (configures and runs wrk)
- **Lua script**: `your_endpoint.lua` (defines HTTP requests)
- **Configuration**: Entry in `benchmark_config.json`

The CI automatically discovers and runs all benchmarks.

## 📝 Step-by-Step

### 1. Create the Lua Script

Create `benchmarks/blocks/blocks.lua`:

```lua
-- Blocks endpoint benchmark
request = function()
    local block_id = math.random(1000000, 5000000)
    return wrk.format("GET", "/blocks/" .. block_id)
end
```

### 2. Create the Runner Script

Create `benchmarks/blocks/init.sh`:

```bash
#!/bin/bash

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
CONFIG_FILE="$PROJECT_ROOT/benchmark_config.json"

SCENARIO="${1:-light_load}"
HARDWARE_PROFILE="${2:-ci_runner}"

# Validate hardware profile
if ! jq -e ".hardware_profiles.\"$HARDWARE_PROFILE\"" "$CONFIG_FILE" > /dev/null; then
    echo "Error: Hardware profile '$HARDWARE_PROFILE' not found"
    exit 1
fi

# Get benchmark configuration
THREADS=$(jq -r ".benchmarks.blocks.scenarios[] | select(.name == \"$SCENARIO\") | .threads" "$CONFIG_FILE")
CONNECTIONS=$(jq -r ".benchmarks.blocks.scenarios[] | select(.name == \"$SCENARIO\") | .connections" "$CONFIG_FILE")
DURATION=$(jq -r ".benchmarks.blocks.scenarios[] | select(.name == \"$SCENARIO\") | .duration" "$CONFIG_FILE")
TIMEOUT=$(jq -r ".benchmarks.blocks.scenarios[] | select(.name == \"$SCENARIO\") | .timeout" "$CONFIG_FILE")

if [ -z "$THREADS" ] || [ "$THREADS" == "null" ]; then
    echo "Error: Scenario '$SCENARIO' not found"
    exit 1
fi

SERVER_HOST=$(jq -r '.server.host' "$CONFIG_FILE")
SERVER_PORT=$(jq -r '.server.port' "$CONFIG_FILE")

echo "Running blocks endpoint benchmark: $SCENARIO"
echo "Configuration: threads=$THREADS, connections=$CONNECTIONS, duration=$DURATION"

# Run wrk benchmark
cd "$SCRIPT_DIR"
wrk -d"$DURATION" -t"$THREADS" -c"$CONNECTIONS" --timeout "${TIMEOUT:-120s}" --latency \
    -s ./blocks.lua "http://$SERVER_HOST:$SERVER_PORT"
```

### 3. Add Configuration

Add your endpoint to `benchmark_config.json`:

```json
{
  "benchmarks": {
    "blocks": {
      "endpoint": "/blocks/:block_id",
      "scenarios": [
        {
          "name": "light_load",
          "description": "Light load - development/CI",
          "threads": 2,
          "connections": 10,
          "duration": "30s",
          "timeout": "60s"
        },
        {
          "name": "medium_load",
          "description": "Medium load - regular testing",
          "threads": 4,
          "connections": 50,
          "duration": "60s",
          "timeout": "120s"
        }
      ]
    }
  }
}
```
