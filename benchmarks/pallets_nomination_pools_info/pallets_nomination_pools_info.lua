-- Pallets nomination-pools info endpoint benchmark script
-- Tests the /v1/pallets/nomination-pools/info endpoint for latency and throughput
--
-- Chain-aware: uses appropriate historical blocks per chain.
-- Nomination pools were migrated off Polkadot relay chain after AHM.

local util = require("util")

local chain = os.getenv("BENCH_CHAIN") or "polkadot"

-- Per-chain block ranges where nomination pools are available
-- TODO: Add block ranges for other chains (kusama, asset-hub-polkadot, etc.)
local chain_blocks = {
    polkadot = { '13088753', '13588753', '14088753', '14588753', '15088753',
                 '15588753', '16088753', '16588753', '17088753', '17588753' },
    kusama   = { '13088753', '13588753', '14088753', '14588753', '15088753',
                 '15588753', '16088753', '16588753', '17088753', '17588753' },
}

local blocks = chain_blocks[chain] or chain_blocks["polkadot"]

-- Build full endpoint list for display
local display_endpoints = {}
for _, block in ipairs(blocks) do
    display_endpoints[#display_endpoints + 1] = util.prefix .. "/pallets/nomination-pools/info?at=" .. block
end
util.print_endpoints(display_endpoints)

local counter = 1

request = function()
    local block = blocks[counter]
    counter = counter + 1
    if counter > #blocks then
        counter = 1
    end
    return wrk.format("GET", util.prefix .. "/pallets/nomination-pools/info?at=" .. block)
end

delay = function()
    -- No delay by default
end

done = util.done()
