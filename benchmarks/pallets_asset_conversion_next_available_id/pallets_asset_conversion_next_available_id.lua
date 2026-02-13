-- Pallets asset-conversion next-available-id endpoint benchmark script
-- Tests the /v1/pallets/asset-conversion/next-available-id endpoint for latency and throughput
-- Note: This endpoint is specific to Asset Hub chains

local util = require("util")

request = function()
    return wrk.format("GET", "/v1/pallets/asset-conversion/next-available-id")
end

done = util.done()
