-- Runtime/spec endpoint benchmark script
-- Tests the /v1/runtime/spec endpoint for latency and throughput
-- Aligned with Sidecar benchmark parameters

local util = require("util")

-- Historical blocks (matching Sidecar)
local endpoints = {
    'spec',
    'spec?at=1000000',
    'spec?at=2000000',
    'spec?at=3000000',
    'spec?at=4000000',
    'spec?at=5000000',
    'spec?at=6000000',
    'spec?at=7000000',
    'spec?at=8000000',
    'spec?at=9000000',
    'spec?at=10000000',
    'spec?at=11000000',
}

local counter = 1

request = function()
    local endpoint = endpoints[counter]
    counter = counter + 1
    if counter > #endpoints then
        counter = 1
    end
    return wrk.format("GET", "/v1/runtime/" .. endpoint)
end

done = util.done()
