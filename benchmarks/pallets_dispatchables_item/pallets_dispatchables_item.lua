-- Pallets dispatchables item endpoint benchmark script
-- Tests the /v1/pallets/{palletId}/dispatchables/{dispatchableId} endpoint for latency and throughput

local util = require("util")

local endpoints = {
    'System/dispatchables/remark?at=11900000',
    'System/dispatchables/set_code?at=11900000',
    'Balances/dispatchables/transfer_allow_death?at=11900000',
    'Balances/dispatchables/force_transfer?at=11900000',
    'Staking/dispatchables/bond?at=11900000',
    'Staking/dispatchables/nominate?at=11900000',
    'Staking/dispatchables/validate?at=11900000',
    'Staking/dispatchables/chill?at=11900000',
    'Session/dispatchables/set_keys?at=11900000',
    'Democracy/dispatchables/propose?at=11900000',
    'Democracy/dispatchables/vote?at=11900000',
    'Treasury/dispatchables/propose_spend?at=11900000',
    'Scheduler/dispatchables/schedule?at=11900000',
    'Timestamp/dispatchables/set?at=11900000',
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
