-- Runtime metadata endpoint benchmark script
-- Tests the /v1/runtime/metadata endpoint for latency and throughput

local util = require("util")

request = function()
    return wrk.format("GET", util.prefix .. "/runtime/metadata")
end

done = util.done()
