-- Capabilities endpoint benchmark script
-- Tests the /v1/capabilities endpoint for latency and throughput

local util = require("util")

request = function()
    return wrk.format("GET", "/v1/capabilities")
end

done = util.done()
