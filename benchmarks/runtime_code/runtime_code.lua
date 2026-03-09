-- Runtime code endpoint benchmark script
-- Tests the /v1/runtime/code endpoint for latency and throughput

local util = require("util")

request = function()
    return wrk.format("GET", "/v1/runtime/code")
end

done = util.done()
