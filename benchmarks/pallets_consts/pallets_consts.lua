-- Pallets consts endpoint benchmark script
-- Tests the /v1/pallets/{palletId}/consts endpoint for latency and throughput

local util = require("util")

local chain = os.getenv("BENCH_CHAIN") or "polkadot"

local endpoints

if chain == "statemint" or chain == "asset-hub-polkadot" then
    -- Polkadot Asset Hub
    endpoints = {
        -- System pallet consts across spec versions
        'System/consts?at=12319018',      -- spec_version 2000007
        'System/consts?at=11896182',      -- spec_version 2000006
        'System/consts?at=11405258',      -- spec_version 2000005
        'System/consts?at=10637835',      -- spec_version 2000003
        'System/consts?at=10344187',      -- spec_version 2000002
        'System/consts?at=10286866',      -- spec_version 2000001
        'System/consts?at=10241801',      -- spec_version 2000000
        'System/consts?at=9784456',       -- spec_version 1007001
        'System/consts?at=9562299',       -- spec_version 1006000
        'System/consts?at=8926584',       -- spec_version 1005001
        'System/consts?at=8548146',       -- spec_version 1004002
        'System/consts?at=8297525',       -- spec_version 1004000
        'System/consts?at=7584039',       -- spec_version 1003004
        'System/consts?at=7342289',       -- spec_version 1003003
        'System/consts?at=7144963',       -- spec_version 1003000
        'System/consts?at=6643079',       -- spec_version 1002006
        'System/consts?at=6593078',       -- spec_version 1002005
        'System/consts?at=6451357',       -- spec_version 1002004
        -- Other Pallets
        'Staking/consts?at=12550000',     -- spec_version 2000007
        'Scheduler/consts?at=10550000',   -- spec_version 2000002
        'Balances/consts?at=10288000',    -- spec_version 2000001
    }
else
    -- Polkadot relay: multiple pallets at a single block
    endpoints = {
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
end

local counter = 1

request = function()
    local endpoint = endpoints[counter]
    counter = counter + 1
    if counter > #endpoints then
        counter = 1
    end
    return wrk.format("GET", util.prefix .. "/pallets/" .. endpoint)
end

done = util.done()
