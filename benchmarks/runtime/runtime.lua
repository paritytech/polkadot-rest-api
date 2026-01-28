-- Runtime/spec endpoint benchmark script
-- Tests the /runtime/spec endpoint for latency and throughput

local util = require("util")

-- Setup the request
request = function()
    return wrk.format("GET", "/v1/runtime/spec")
end

-- No delay between requests for maximum throughput
delay = function()
    -- No delay by default
end

-- Signal completion with statistics
done = util.done()

