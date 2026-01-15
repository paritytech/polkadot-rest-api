-- Node network endpoint benchmark script
-- Tests the /v1/node/network endpoint for latency and throughput

local util = require("../util")

-- Setup the request
request = function()
    return wrk.format("GET", "/v1/node/network")
end

-- No delay between requests for maximum throughput
delay = function()
    -- No delay by default
end

-- Signal completion with statistics
done = util.done()
