-- Node transaction pool endpoint benchmark script
-- Tests the /v1/node/transaction-pool endpoint for latency and throughput
-- Aligned with Sidecar benchmark parameters

local util = require("util")

-- Transaction pool with and without fee calculation (matching Sidecar)
local paths = {
    '/node/transaction-pool',
    '/node/transaction-pool?includeFee=true',
}

local counter = 1

request = function()
    local path = paths[counter]
    counter = counter + 1
    if counter > #paths then
        counter = 1
    end
    return wrk.format("GET", util.prefix .. path)
end

done = util.done()
