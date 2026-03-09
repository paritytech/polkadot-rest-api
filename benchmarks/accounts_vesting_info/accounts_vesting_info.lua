-- Accounts vesting-info endpoint benchmark script
-- Tests the /v1/accounts/{accountId}/vesting-info endpoint for latency and throughput
-- Aligned with Sidecar benchmark parameters

local util = require("util")

-- Multiple accounts with historical blocks (matching Sidecar)
local endpoints = {
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

local counter = 1

request = function()
    local endpoint = endpoints[counter]
    counter = counter + 1
    if counter > #endpoints then
        counter = 1
    end
    return wrk.format("GET", "/v1/accounts/" .. endpoint)
end

done = util.done()
