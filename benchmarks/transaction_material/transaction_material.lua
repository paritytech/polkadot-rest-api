-- Transaction material endpoint benchmark script
-- Tests the /v1/transaction/material endpoint for latency and throughput
-- Aligned with Sidecar benchmark parameters

local util = require("util")

-- Historical blocks (matching Sidecar)
local endpoints = {
    'material',
    'material?at=1000000',
    'material?at=2000000',
    'material?at=3000000',
    'material?at=4000000',
    'material?at=5000000',
    'material?at=6000000',
    'material?at=7000000',
    'material?at=8000000',
    'material?at=9000000',
    'material?at=10000000',
    'material?at=11000000',
}

local counter = 1

request = function()
    local endpoint = endpoints[counter]
    counter = counter + 1
    if counter > #endpoints then
        counter = 1
    end
    return wrk.format("GET", "/v1/transaction/" .. endpoint)
end

done = util.done()
