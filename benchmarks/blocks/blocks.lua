-- Blocks endpoint benchmark script
-- Tests the /blocks/:blockId endpoint for latency and throughput

local util = require("util")

-- Generate random block IDs in a realistic range
-- Adjust this range based on your chain's block height
request = function()
    local block_id = math.random(1000000, 5000000)
    return wrk.format("GET", "/v1/blocks/" .. block_id)
end

-- No delay between requests for maximum throughput
delay = function()
    -- No delay by default
end

-- Signal completion with statistics
done = util.done()
