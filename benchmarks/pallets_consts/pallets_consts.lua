-- Pallets consts endpoint benchmark script
-- Tests the /v1/pallets/{palletId}/consts endpoint for latency and throughput

local util = require("util")

local endpoints = {
    'System/consts?at=11900000',
    'Balances/consts?at=11900000',
    'Staking/consts?at=11900000',
    'Timestamp/consts?at=11900000',
    'TransactionPayment/consts?at=11900000',
    'Democracy/consts?at=11900000',
    'Treasury/consts?at=11900000',
    'Scheduler/consts?at=11900000',
    'Indices/consts?at=11900000',
    'Session/consts?at=11900000',
    'Grandpa/consts?at=11900000',
    'ImOnline/consts?at=11900000',
    'TechnicalCommittee/consts?at=11900000',
    'Council/consts?at=11900000',
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
