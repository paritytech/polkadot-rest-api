-- Accounts vesting-info endpoint benchmark script
-- Tests the /v1/accounts/{accountId}/vesting-info endpoint for latency and throughput
-- Aligned with Sidecar benchmark parameters
--
-- Chain-aware: uses chain-specific accounts and blocks.

local util = require("util")

local chain = os.getenv("BENCH_CHAIN") or "polkadot"

local endpoints = {}

if chain == "asset-hub-polkadot" or chain == "statemint" then
    -- Asset Hub Polkadot-specific accounts and blocks
    endpoints = {
        '15HpzYLuTuHGAo4pjG3uUrUgKAQnBRRAd62ZdRaujAgXjiQa/vesting-info?at=12732847',    -- spec_version 2000007, vesting: 4 items
        '16TaSR2xDjgesAn11WYyfb9eLia9BRCrLxn311MyCYjar8h7/vesting-info?at=11197835',    -- spec_version 2000003, vesting: 5 items
    }
else
    -- Polkadot relay: accounts with historical blocks (matching Sidecar)
    endpoints = {
        '15aKvwRqGVAwuBMaogtQXhuz9EQqUWsZJSAzomyb5xYwgBXA/vesting-info?at=1448',
        '123PewK4ZYcX7Do8PKzP4KyYbLKMQAAA3EhhZcnBDrxAuidt/vesting-info?at=10254',
        '16FQxY2L9GbBoE1jYCDRUkJRroYMu5FsKRQfFi29xueu1egj/vesting-info?at=111170',
        '1BjwMkGfudp4eVAMpqv6CHZJxGsLFkqQv5oaZT9gWc5o7hn/vesting-info?at=213327',
        '1BjwMkGfudp4eVAMpqv6CHZJxGsLFkqQv5oaZT9gWc5o7hn/vesting-info?at=2413527',
        '123PewK4ZYcX7Do8PKzP4KyYbLKMQAAA3EhhZcnBDrxAuidt/vesting-info?at=4353425',
        '16FQxY2L9GbBoE1jYCDRUkJRroYMu5FsKRQfFi29xueu1egj/vesting-info?at=6413249',
        '15aKvwRqGVAwuBMaogtQXhuz9EQqUWsZJSAzomyb5xYwgBXA/vesting-info?at=7232861',
        '15aKvwRqGVAwuBMaogtQXhuz9EQqUWsZJSAzomyb5xYwgBXA/vesting-info?at=8000000',
        '15aKvwRqGVAwuBMaogtQXhuz9EQqUWsZJSAzomyb5xYwgBXA/vesting-info?at=8320000',
        '15aKvwRqGVAwuBMaogtQXhuz9EQqUWsZJSAzomyb5xYwgBXA/vesting-info?at=8500000',
        '15aKvwRqGVAwuBMaogtQXhuz9EQqUWsZJSAzomyb5xYwgBXA/vesting-info?at=9000000',
        '15aKvwRqGVAwuBMaogtQXhuz9EQqUWsZJSAzomyb5xYwgBXA/vesting-info?at=9500000',
    }
end

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
