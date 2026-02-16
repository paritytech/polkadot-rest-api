-- Pallets asset-conversion liquidity-pools endpoint benchmark script
-- Tests the /v1/pallets/asset-conversion/liquidity-pools endpoint for latency and throughput
-- Note: This endpoint is specific to Asset Hub chains

local util = require("util")

request = function()
    return wrk.format("GET", "/v1/pallets/asset-conversion/liquidity-pools")
end

done = util.done()
