-- RC transaction material endpoint benchmark script
-- Tests the /v1/rc/transaction/material endpoint for latency and throughput
-- Note: This endpoint is only available on parachains

local util = require("util")

request = function()
    return wrk.format("GET", util.prefix .. "/rc/transaction/material")
end

done = util.done()
