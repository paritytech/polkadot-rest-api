-- Pallets storage item endpoint benchmark script
-- Tests the /v1/pallets/{palletId}/storage/{storageItemId} endpoint for latency and throughput
-- Aligned with Sidecar benchmark parameters

local util = require("util")

-- Multiple pallets/storage items with historical blocks (matching Sidecar)
local endpoints = {
    'Staking/storage/bonded?keys[]=16CxQy9MGAUa4ubbQ6dcc3BWzKK9LuNN7AaZxtQJ5Q4vefbo&at=11988000',
    'Staking/storage/bonded?keys[]=16CxQy9MGAUa4ubbQ6dcc3BWzKK9LuNN7AaZxtQJ5Q4vefbo&at=11800000',
    'Staking/storage/MaxValidatorsCount?at=11988000',
    'Staking/storage/MaxValidatorsCount?at=11800000',
    'Staking/storage/BondedEras?at=11988000',
    'Staking/storage/BondedEras?at=11800000',
    'Balances/storage/TotalIssuance?at=11988000',
    'Balances/storage/TotalIssuance?at=11800000',
    'Balances/storage/Locks?keys[]=16ZL8yLyXv3V3L3z9ofR1ovFLziyXaN1DPq4yffMAZ9czzBD&at=11988000',
    'Balances/storage/Locks?keys[]=16ZL8yLyXv3V3L3z9ofR1ovFLziyXaN1DPq4yffMAZ9czzBD&at=11800000',
    'System/storage/Account?keys[]=16ZL8yLyXv3V3L3z9ofR1ovFLziyXaN1DPq4yffMAZ9czzBD&at=11988000',
    'System/storage/Account?keys[]=16ZL8yLyXv3V3L3z9ofR1ovFLziyXaN1DPq4yffMAZ9czzBD&at=11800000',
    'System/storage/ParentHash',
    'System/storage/ParentHash',
    'Democracy/storage/LastTabledWasExternal?at=11988000',
    'Democracy/storage/LastTabledWasExternal?at=11800000',
    'Democracy/storage/StorageVersion?at=11988000',
    'Democracy/storage/StorageVersion?at=11800000',
    'Treasury/storage/ProposalCount?at=11988000',
    'Treasury/storage/ProposalCount?at=11800000',
    'Treasury/storage/Approvals?at=11988000',
    'Treasury/storage/Approvals?at=11800000',
    'TransactionPayment/storage/NextFeeMultiplier?at=11988000',
    'TransactionPayment/storage/NextFeeMultiplier?at=11800000',
    'TransactionPayment/storage/StorageVersion?at=11988000',
    'TransactionPayment/storage/StorageVersion?at=11800000',
    'PhragmenElection/storage/members?at=11988000',
    'PhragmenElection/storage/members?at=11800000',
    'PhragmenElection/storage/ElectionRounds?at=11988000',
    'PhragmenElection/storage/ElectionRounds?at=11800000',
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
