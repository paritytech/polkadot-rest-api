-- Accounts staking-payouts endpoint benchmark script
-- Tests the /v1/accounts/{accountId}/staking-payouts endpoint for latency and throughput
-- Aligned with Sidecar benchmark parameters
--
-- Chain-aware: uses appropriate historical blocks per chain.
-- Staking was migrated off Polkadot relay chain after AHM, so queries at head fail.

local util = require("util")

local chain = os.getenv("BENCH_CHAIN") or "polkadot"

-- Each entry is a verified account+block pair where staking payouts exist
local endpoints = {}

if chain == "asset-hub-polkadot" or chain == "statemint" then
    endpoints = {
        -- different accounts @ different blocks @ different spec_versions
        '112E6RqiX78Lm5QPBcVz2QsKVVXSDoHA8JJxu5P3iay15HrF/staking-payouts?at=13185919',     -- spec_version 2000007, 1 payout: unclaimed
        '162irz3JWpH3dNLo5vUdEz7RTzStkKJyjeYSmSTQE6yYGi1K/staking-payouts?at=11896484',     -- spec_version 2000006, 4 payouts: all claimed
        '12LdngMrL3xmSSj6bCpAqprZBWWkHBfYdv47RxWMPZtWzuwK/staking-payouts?at=11430473',     -- spec_version 2000005, 5 payouts: all claimed
        '12pw22Qyy3o28BLshjce9yrSMs3fhSiLsAjqLPAzGktbXYV7/staking-payouts?at=11096339',     -- spec_version 2000003, 4 payouts: 3 claimed, 1 unclaimed
        '14n7KYKKjZohVapvNYnWdCVu7oCJMjyWHqehGcC11wE7dKWv/staking-payouts?at=10401948',     -- spec_version 2000002, 4 payouts: 3 claimed, 1 unclaimed
        '13Agjkd5embbwk3rZ1zrf66E2TbL11gNgWGaa6pTj7Ar41Nr/staking-payouts?at=10306695',     -- spec_version 2000001, 2 payouts: 1 claimed, 1 unclaimed
        '13Xo4xYdMRQQoWD7TA9yCDHG1BXqQFvnjeGvU6LHhRhUsBhQ/staking-payouts?at=10265744',     -- spec_version 2000000, 8 payouts: 2 claimed, 6 unclaimed
    }
else
    -- Polkadot relay / Kusama: verified account+block pairs
    endpoints = {
        '12WLDL2AXoH3MHr1xj8K4m9rCcRKSWKTUz8A4mX3ah5khJBn/staking-payouts?at=27723608',
        '12WLDL2AXoH3MHr1xj8K4m9rCcRKSWKTUz8A4mX3ah5khJBn/staking-payouts?at=17723608',
        '14bUYpiF2oxVpmXDnFxBipSi4m9zYBThMZoLpY8bRQrPQNG1/staking-payouts?at=27723608',
        '15omhU2Gi3ounztEznJ9Bj49dvoPhSi9wN1M7uoniTt9F72d/staking-payouts?at=27737961',
        '16Rtxs1CuR6EgQEsi2yJ4YFRFRwRakXShMCAuGW2MKRwpjHo/staking-payouts?at=27737969',
        '13S541dQ5NXFCxSBqFUFghkCfUU6LsZUVem7z2tfvsJwWFys/staking-payouts?at=28395380',
        '12R1iRVuxLUHU1v3DHNxbvA2SNq2KbmL3FnsQTCQ2Sppngzx/staking-payouts?at=27750968',
        '1737bipUqNUHYjUB5HCezyYqto5ZjFiMSXNAX8fWktnD5AS/staking-payouts?at=27752311',
        '12YP2b7L7gcHabZqE7vJMyF9eSZA9W68gnvb8BzTYx4MUxRo/staking-payouts?at=27752310',
        '14DZ3GPuvb8Z9z4UgxV1ikC7UoypLXWYDy77MjoQJ3qMByW2/staking-payouts?at=7000000',
        '16kDoP9nFg4KUkjb3SSNnkmibKs1spmakxy6JLVCAFxTeSa3/staking-payouts?at=6500000',
        '12pECvQp8dESMAYfQFV4A23aCcUyWWN6MftLizH2wxVxXZJW/staking-payouts?at=27750968',
    }
end

util.print_endpoints(endpoints)

local counter = 1

request = function()
    local endpoint = endpoints[counter]
    counter = counter + 1
    if counter > #endpoints then
        counter = 1
    end
    return wrk.format("GET", util.prefix .. "/accounts/" .. endpoint)
end

done = util.done()
