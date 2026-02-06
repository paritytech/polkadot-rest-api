-- Pallets errors endpoint benchmark script
-- Tests the /v1/pallets/{palletId}/errors endpoint for latency and throughput
-- Aligned with Sidecar benchmark parameters

local util = require("util")

-- Multiple pallets with historical blocks (matching Sidecar)
local endpoints = {
    'System/errors?at=11900000',
    'Scheduler/errors?at=11900000',
    'Preimage/errors?at=11900000',
    'Babe/errors?at=11900000',
    'Indices/errors?at=11900000',
    'Balances/errors?at=11900000',
    'Authorship/errors?at=11900000',
    'Staking/errors?at=11900000',
    'Session/errors?at=11900000',
    'Grandpa/errors?at=11900000',
    'ImOnline/errors?at=11900000',
    'Democracy/errors?at=11900000',
    'TechnicalCommittee/errors?at=11900000',
    'Council/errors?at=11900000',
}

local counter = 1

request = function()
    local endpoint = endpoints[counter]
    counter = counter + 1
    if counter > #endpoints then
        counter = 1
    end
    return wrk.format("GET", "/v1/pallets/" .. endpoint)
end

done = util.done()
