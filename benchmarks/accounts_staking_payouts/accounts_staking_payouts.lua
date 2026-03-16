-- Accounts staking-payouts endpoint benchmark script
-- Tests the /v1/accounts/{accountId}/staking-payouts endpoint for latency and throughput
-- Aligned with Sidecar benchmark parameters
--
-- Chain-aware: uses appropriate historical blocks per chain.
-- Staking was migrated off Polkadot relay chain after AHM, so queries at head fail.

local util = require("util")

local chain = os.getenv("BENCH_CHAIN") or "polkadot"

-- Per-chain block ranges where staking payouts are available
-- TODO: Add block ranges for other chains (kusama, asset-hub-polkadot, etc.)
local chain_blocks = {
    polkadot = { '27723608', '17723608', '27737961', '27737969', '28395380',
                 '27750968', '27752311', '27752310', '7000000', '6500000' },
    kusama   = { '11000000', '10500000', '10000000', '9500000', '9000000',
                 '8500000', '8000000', '7500000', '7000000', '6500000' },
}

local blocks = chain_blocks[chain] or chain_blocks["polkadot"]

-- Multiple validator accounts (matching Sidecar)
local accounts = {
    '12WLDL2AXoH3MHr1xj8K4m9rCcRKSWKTUz8A4mX3ah5khJBn', -- Polkadot @ 27723608, 17723608
    '14bUYpiF2oxVpmXDnFxBipSi4m9zYBThMZoLpY8bRQrPQNG1', -- Polkadot
    '15omhU2Gi3ounztEznJ9Bj49dvoPhSi9wN1M7uoniTt9F72d', -- Polkadot @ 27737961
    '16Rtxs1CuR6EgQEsi2yJ4YFRFRwRakXShMCAuGW2MKRwpjHo', -- Polkadot @ 27737969
    '13S541dQ5NXFCxSBqFUFghkCfUU6LsZUVem7z2tfvsJwWFys', -- Polkadot @ 28395380
    '12R1iRVuxLUHU1v3DHNxbvA2SNq2KbmL3FnsQTCQ2Sppngzx', -- Polkadot @ 27750968
    '1737bipUqNUHYjUB5HCezyYqto5ZjFiMSXNAX8fWktnD5AS',  -- Polkadot @ 27752311
    '12YP2b7L7gcHabZqE7vJMyF9eSZA9W68gnvb8BzTYx4MUxRo', -- Polkadot @ 27752310
    '14DZ3GPuvb8Z9z4UgxV1ikC7UoypLXWYDy77MjoQJ3qMByW2',
    '16kDoP9nFg4KUkjb3SSNnkmibKs1spmakxy6JLVCAFxTeSa3',
    '12pECvQp8dESMAYfQFV4A23aCcUyWWN6MftLizH2wxVxXZJW',
}

-- Build full endpoint list for display
local display_endpoints = {}
for _, account in ipairs(accounts) do
    for _, block in ipairs(blocks) do
        display_endpoints[#display_endpoints + 1] = "/v1/accounts/" .. account .. "/staking-payouts?at=" .. block
    end
end
util.print_endpoints(display_endpoints)

local account_counter = 1
local block_counter = 1

request = function()
    local account = accounts[account_counter]
    local block = blocks[block_counter]

    account_counter = account_counter + 1
    if account_counter > #accounts then
        account_counter = 1
        block_counter = block_counter + 1
        if block_counter > #blocks then
            block_counter = 1
        end
    end

    return wrk.format("GET", "/v1/accounts/" .. account .. "/staking-payouts?at=" .. block)
end

done = util.done()
