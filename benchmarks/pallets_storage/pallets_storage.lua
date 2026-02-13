-- Pallets storage endpoint benchmark script
-- Tests the /v1/pallets/{palletId}/storage endpoint for latency and throughput
-- Aligned with Sidecar benchmark parameters

local util = require("util")

-- Multiple pallets at two block heights (matching Sidecar)
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
    'Democracy/storage?at=11900000',
    'TechnicalCommittee/storage?at=11900000',
    'Council/storage?at=11900000',
    'PhragmenElection/storage?at=11900000',
    'Treasury/storage?at=11900000',
    'Claims/storage?at=11900000',
    'System/storage?at=6000000',
    'Scheduler/storage?at=6000000',
    'Babe/storage?at=6000000',
    'Timestamp/storage?at=6000000',
    'Indices/storage?at=6000000',
    'Balances/storage?at=6000000',
    'TransactionPayment/storage?at=6000000',
    'Authorship/storage?at=6000000',
    'Staking/storage?at=6000000',
    'Offences/storage?at=6000000',
    'Session/storage?at=6000000',
    'Grandpa/storage?at=6000000',
    'ImOnline/storage?at=6000000',
    'Democracy/storage?at=6000000',
    'TechnicalCommittee/storage?at=6000000',
    'Council/storage?at=6000000',
    'PhragmenElection/storage?at=6000000',
    'Treasury/storage?at=6000000',
    'Claims/storage?at=6000000',
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
