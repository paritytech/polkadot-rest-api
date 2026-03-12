-- Coretime info endpoint benchmark script
-- Tests the /v1/coretime/info endpoint for latency and throughput

local util = require("util")

request = function()
    return wrk.format("GET", util.prefix .. "/coretime/info")
end

done = util.done()
