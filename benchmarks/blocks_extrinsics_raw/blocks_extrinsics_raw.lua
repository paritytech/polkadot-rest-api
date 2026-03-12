-- Blocks extrinsics-raw endpoint benchmark script
-- Tests the /v1/blocks/{blockId}/extrinsics-raw endpoint for latency and throughput
--
-- Chain-aware: uses chain-specific blocks.

local util = require("util")

local chain = os.getenv("BENCH_CHAIN") or "polkadot"

local blocks = {}

if chain == "asset-hub-polkadot" or chain == "statemint" then
    -- Asset Hub Polkadot-specific blocks
    blocks = {
        -- Blocks @ different spec versions
        '12319018',      -- spec_version 2000007
        '11896182',      -- spec_version 2000006
        '11405258',      -- spec_version 2000005
        '10637835',      -- spec_version 2000003
        '10344187',      -- spec_version 2000002
        '10286866',      -- spec_version 2000001
        '10241801',      -- spec_version 2000000
        '9784456',       -- spec_version 1007001
        '9562299',       -- spec_version 1006000
        '8926584',       -- spec_version 1005001
        '8548146',       -- spec_version 1004002
        '8297525',       -- spec_version 1004000
        '7584039',       -- spec_version 1003004
        '7342289',       -- spec_version 1003003
        '7144963',       -- spec_version 1003000
        '6643079',       -- spec_version 1002006
        '6593078',       -- spec_version 1002005
        '6451357',       -- spec_version 1002004
        -- Blocks with more extrinsics
        '10259193',      -- 52 extrinsics @ spec_version 2000000
        '10259183',      -- 52 extrinsics @ spec_version 2000000
        '10872783',      -- 53 extrinsics @ spec_version 2000003
        '10873066',      -- 47 extrinsics @ spec_version 2000003
        '10873360',      -- 47 extrinsics @ spec_version 2000003
        '10873648',      -- 50 extrinsics @ spec_version 2000003
        '11038658',      -- 46 extrinsics @ spec_version 2000003
        '11038369',      -- 46 extrinsics @ spec_version 2000003
        '11038074',      -- 46 extrinsics @ spec_version 2000003
        '13255796',      -- 156 extrinsics @ spec_version 2000007
        '13254859'       -- 45 extrinsics @ spec_version 2000007
    }
else
    -- Polkadot relay: historical blocks (matching Sidecar)
    blocks = {
        '28831',
        '29258',
        '188836',
        '197681',
        '199405',
        '200732',
        '214264',
        '214576',
        '243601',
        '244358',
        '287352',
        '300532',
        '301569',
        '302396',
    }
end

local counter = 1

request = function()
    local block = blocks[counter]
    counter = counter + 1
    if counter > #blocks then
        counter = 1
    end
    return wrk.format("GET", util.prefix .. "/blocks/" .. block .. "/extrinsics-raw")
end

done = util.done()
