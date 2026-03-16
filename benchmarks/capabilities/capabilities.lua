-- Capabilities endpoint benchmark script
-- Tests the /v1/capabilities endpoint for latency and throughput

local util = require("util")

request = function()
    return wrk.format("GET", util.prefix .. "/capabilities")
end

done = util.done()
