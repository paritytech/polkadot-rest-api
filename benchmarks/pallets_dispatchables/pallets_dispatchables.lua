-- Pallets dispatchables endpoint benchmark script
-- Tests the /v1/pallets/{palletId}/dispatchables endpoint for latency and throughput

local util = require("util")

local endpoints = {
    'System/dispatchables?at=11900000',
    'Balances/dispatchables?at=11900000',
    'Staking/dispatchables?at=11900000',
    'Session/dispatchables?at=11900000',
    'Grandpa/dispatchables?at=11900000',
    'Democracy/dispatchables?at=11900000',
    'Treasury/dispatchables?at=11900000',
    'Scheduler/dispatchables?at=11900000',
    'Preimage/dispatchables?at=11900000',
    'Indices/dispatchables?at=11900000',
    'Timestamp/dispatchables?at=11900000',
    'ImOnline/dispatchables?at=11900000',
    'TechnicalCommittee/dispatchables?at=11900000',
    'Council/dispatchables?at=11900000',
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
