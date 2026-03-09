-- Node transaction pool endpoint benchmark script
-- Tests the /v1/node/transaction-pool endpoint for latency and throughput
-- Aligned with Sidecar benchmark parameters

local util = require("util")

-- Transaction pool with and without fee calculation (matching Sidecar)
local endpoints = {
    '/v1/node/transaction-pool',
    '/v1/node/transaction-pool?includeFee=true',
}

local counter = 1

request = function()
    local endpoint = endpoints[counter]
    counter = counter + 1
    if counter > #endpoints then
        counter = 1
    end
    return wrk.format("GET", endpoint)
end

done = util.done()
