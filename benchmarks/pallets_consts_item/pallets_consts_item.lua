-- Pallets consts item endpoint benchmark script
-- Tests the /v1/pallets/{palletId}/consts/{constantItemId} endpoint for latency and throughput

local util = require("util")

local endpoints = {
    'System/consts/BlockWeights?at=11900000',
    'System/consts/BlockLength?at=11900000',
    'System/consts/BlockHashCount?at=11900000',
    'Balances/consts/ExistentialDeposit?at=11900000',
    'Balances/consts/MaxLocks?at=11900000',
    'Staking/consts/MaxNominations?at=11900000',
    'Staking/consts/BondingDuration?at=11900000',
    'Staking/consts/SessionsPerEra?at=11900000',
    'Timestamp/consts/MinimumPeriod?at=11900000',
    'TransactionPayment/consts/OperationalFeeMultiplier?at=11900000',
    'Democracy/consts/VotingPeriod?at=11900000',
    'Democracy/consts/EnactmentPeriod?at=11900000',
    'Treasury/consts/SpendPeriod?at=11900000',
    'Treasury/consts/ProposalBond?at=11900000',
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
