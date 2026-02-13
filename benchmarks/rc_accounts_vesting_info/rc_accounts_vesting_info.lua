-- RC accounts vesting-info endpoint benchmark script
-- Tests the /v1/rc/accounts/{accountId}/vesting-info endpoint for latency and throughput
-- Note: This endpoint is only available on parachains

local util = require("util")

local endpoints = {
    '15aKvwRqGVAwuBMaogtQXhuz9EQqUWsZJSAzomyb5xYwgBXA/vesting-info',
    '123PewK4ZYcX7Do8PKzP4KyYbLKMQAAA3EhhZcnBDrxAuidt/vesting-info',
    '16FQxY2L9GbBoE1jYCDRUkJRroYMu5FsKRQfFi29xueu1egj/vesting-info',
    '1BjwMkGfudp4eVAMpqv6CHZJxGsLFkqQv5oaZT9gWc5o7hn/vesting-info',
    '1KvKReVmUiTc2LW2a4qyHsaJJ9eE9LRsywZkMk5hyBeyHgw/vesting-info',
    '14Kq2Gt4buLr8XgRQmLtbWLHkejmhvGhiZDqLEzWcbe7jQTU/vesting-info',
    '15kUt2i86LHRWCkE3D9Bg1HZAoc2smhn1fwPzDERTb1BXAkX/vesting-info',
    '13BN4WksoyexwDWhGsMMUbU5okehD19GzdyqL4DMPR2KkQpP/vesting-info',
    '16MNMABGfPChG1RHxeb2YzoWUrX22G5CPnvarkmDJXzsZVRV/vesting-info',
    '13KJ3t8w1CKMkXCmZ6s3VwdWo4h747kXE88ZNh6rCBTvojmM/vesting-info',
}

local counter = 1

request = function()
    local endpoint = endpoints[counter]
    counter = counter + 1
    if counter > #endpoints then
        counter = 1
    end
    return wrk.format("GET", "/v1/rc/accounts/" .. endpoint)
end

done = util.done()
