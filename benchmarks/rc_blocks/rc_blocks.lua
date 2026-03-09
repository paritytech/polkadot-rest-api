-- RC blocks endpoint benchmark script
-- Tests the /v1/rc/blocks/{blockId} endpoint for latency and throughput
-- Note: This endpoint is only available on parachains

local util = require("util")

request = function()
    local block_id = math.random(1000000, 5000000)
    return wrk.format("GET", "/v1/rc/blocks/" .. block_id)
end

done = util.done()
