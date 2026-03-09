-- Accounts convert endpoint benchmark script
-- Tests the /v1/accounts/{accountId}/convert endpoint for latency and throughput
-- Aligned with Sidecar benchmark parameters

local util = require("util")

-- Hex account address (matching Sidecar)
local endpoints = {
    '0xde1894014026720b9918b1b21b488af8a0d4f15953621233830946ec0b4d7b75/convert',
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
