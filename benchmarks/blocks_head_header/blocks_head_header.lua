-- Blocks head header endpoint benchmark script
-- Tests the /blocks/head/header endpoint for latency and throughput

local util = require("util")

-- Setup the request
request = function()
    return wrk.format("GET", "/v1/blocks/head/header")
end

-- No delay between requests for maximum throughput
delay = function()
    -- No delay by default
end

-- Signal completion with statistics
done = util.done()
