-- Node transaction pool endpoint benchmark script
-- Tests the /v1/node/transaction-pool endpoint for latency and throughput

local util = require("util")

-- Setup the request
request = function()
    return wrk.format("GET", "/v1/node/transaction-pool")
end

-- No delay between requests for maximum throughput
delay = function()
    -- No delay by default
end

-- Signal completion with statistics
done = util.done()
