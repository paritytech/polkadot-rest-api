-- Coretime leases endpoint benchmark script
-- Tests the /v1/coretime/leases endpoint for latency and throughput
-- Note: This endpoint is specific to Coretime chains

local util = require("util")

request = function()
    return wrk.format("GET", util.prefix .. "/coretime/leases")
end

done = util.done()
