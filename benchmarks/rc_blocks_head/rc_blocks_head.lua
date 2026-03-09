-- RC blocks head endpoint benchmark script
-- Tests the /v1/rc/blocks/head endpoint for latency and throughput
-- Note: This endpoint is only available on parachains

local util = require("util")

request = function()
    return wrk.format("GET", "/v1/rc/blocks/head")
end

done = util.done()
