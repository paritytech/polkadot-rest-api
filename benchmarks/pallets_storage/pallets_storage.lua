-- Pallets storage endpoint benchmark script
-- Tests the /v1/pallets/{palletId}/storage endpoint for latency and throughput
-- Aligned with Sidecar benchmark parameters

local util = require("util")

-- Multiple pallets with historical blocks (matching Sidecar)
local endpoints = {
    'System/storage?at=11900000',
    'Scheduler/storage?at=11900000',
    'Preimage/storage?at=11900000',
    'Babe/storage?at=11900000',
    'Timestamp/storage?at=11900000',
    'Indices/storage?at=11900000',
    'Balances/storage?at=11900000',
    'TransactionPayment/storage?at=11900000',
    'Authorship/storage?at=11900000',
    'Staking/storage?at=11900000',
    'Offences/storage?at=11900000',
    'Session/storage?at=11900000',
    'Grandpa/storage?at=11900000',
    'ImOnline/storage?at=11900000',
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
