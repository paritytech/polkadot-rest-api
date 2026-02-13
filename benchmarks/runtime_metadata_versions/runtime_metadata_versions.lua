-- Runtime metadata versions endpoint benchmark script
-- Tests the /v1/runtime/metadata/versions endpoint for latency and throughput

local util = require("util")

request = function()
    return wrk.format("GET", "/v1/runtime/metadata/versions")
end

done = util.done()
