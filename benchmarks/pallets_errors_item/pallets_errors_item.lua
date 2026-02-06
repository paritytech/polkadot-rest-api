-- Pallets errors item endpoint benchmark script
-- Tests the /v1/pallets/{palletId}/errors/{errorItemId} endpoint for latency and throughput
-- Aligned with Sidecar benchmark parameters

local util = require("util")

-- Multiple pallets/errors with historical blocks (matching Sidecar)
local endpoints = {
    'Democracy/errors/ProposalMissing?at=11900000',
    'System/errors/InvalidSpecName?at=11900000',
    'Scheduler/errors/FailedToSchedule?at=11900000',
    'Balances/errors/VestingBalance?at=11900000',
    'Democracy/errors/ProposalMissing?at=10000000',
    'System/errors/InvalidSpecName?at=10000000',
    'Scheduler/errors/FailedToSchedule?at=10000000',
    'Balances/errors/VestingBalance?at=10000000',
    'Democracy/errors/ProposalMissing?at=9000000',
    'System/errors/InvalidSpecName?at=9000000',
    'Scheduler/errors/FailedToSchedule?at=9000000',
    'Balances/errors/VestingBalance?at=9000000',
    'Democracy/errors/ProposalMissing?at=8000000',
    'System/errors/InvalidSpecName?at=8000000',
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
