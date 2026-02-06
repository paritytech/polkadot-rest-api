-- Blocks header endpoint benchmark script
-- Tests the /v1/blocks/{blockId}/header endpoint for latency and throughput
-- Aligned with Sidecar benchmark parameters

local util = require("util")

-- Historical blocks (matching Sidecar)
local endpoints = {
    '28831/header',
    '29258/header',
    '188836/header',
    '197681/header',
    '199405/header',
    '200732/header',
    '214264/header',
    '214576/header',
    '243601/header',
    '244358/header',
    '287352/header',
    '300532/header',
    '301569/header',
    '302396/header',
}

local counter = 1

request = function()
    local endpoint = endpoints[counter]
    counter = counter + 1
    if counter > #endpoints then
        counter = 1
    end
    return wrk.format("GET", "/v1/blocks/" .. endpoint)
end

done = util.done()
