-- Pallets nomination-pools by ID endpoint benchmark script
-- Tests the /v1/pallets/nomination-pools/{poolId} endpoint for latency and throughput
-- Aligned with Sidecar benchmark parameters

local util = require("util")

-- Multiple pools with historical blocks (matching Sidecar)
local endpoints = {
    '1?at=13088753',
    '2?at=13088753',
    '3?at=13088753',
    '4?at=13088753',
    '2?at=13588753',
    '3?at=13588753',
    '4?at=13588753',
    '1?at=14088753',
    '2?at=14088753',
    '3?at=14088753',
    '4?at=14088753',
}

local counter = 1

request = function()
    local endpoint = endpoints[counter]
    counter = counter + 1
    if counter > #endpoints then
        counter = 1
    end
    return wrk.format("GET", "/v1/pallets/nomination-pools/" .. endpoint)
end

done = util.done()
