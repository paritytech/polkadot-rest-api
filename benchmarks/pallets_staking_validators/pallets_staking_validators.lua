-- Pallets staking validators endpoint benchmark script
-- Tests the /v1/pallets/staking/validators endpoint for latency and throughput
-- Aligned with Sidecar benchmark parameters
--
-- Chain-aware: uses chain-specific blocks.

local util = require("util")

local chain = os.getenv("BENCH_CHAIN") or "polkadot"

local blocks = {}

if chain == "asset-hub-polkadot" or chain == "statemint" then
    -- Asset Hub Polkadot-specific blocks (matching pallets_staking_progress.lua)
    blocks = {
        '13185919',     -- spec_version 2000007
        '11896484',     -- spec_version 2000006
        '11430473',     -- spec_version 2000005
        '11096339',     -- spec_version 2000003
        '10401948',     -- spec_version 2000002
        '10306695',     -- spec_version 2000001
        '10265744',     -- spec_version 2000000
    }
else
    -- Polkadot relay: historical blocks (matching Sidecar)
    blocks = {
        '11000000',
        '10500000',
        '10000000',
        '9500000',
        '9000000',
        '8500000',
        '8000000',
        '7500000',
        '7000000',
        '6500000',
        '6000000',
        '5500000',
        '5000000',
        '4500000',
        '4000000',
        '3500000',
        '3000000',
        '2000000',
        '1000000',
    }
end

local counter = 1

request = function()
    local block = blocks[counter]
    counter = counter + 1
    if counter > #blocks then
        counter = 1
    end
    return wrk.format("GET", util.prefix .. "/pallets/staking/validators?at=" .. block)
end

done = util.done()
