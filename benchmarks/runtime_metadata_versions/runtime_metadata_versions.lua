-- Runtime metadata versions endpoint benchmark script
-- Tests the /v1/runtime/metadata/versions endpoint for latency and throughput

local util = require("util")

request = function()
    return wrk.format("GET", util.prefix .. "/runtime/metadata/versions")
end

done = util.done()
