-- Blocks extrinsics endpoint benchmark script
-- Tests the /v1/blocks/{blockId}/extrinsics/{extrinsicIndex} endpoint for latency and throughput
-- Aligned with Sidecar benchmark parameters

local util = require("util")

-- Historical blocks with extrinsics (matching Sidecar)
local endpoints = {
    '28831/extrinsics/0',
    '29258/extrinsics/0',
    '188836/extrinsics/0',
    '197681/extrinsics/0',
    '199405/extrinsics/0',
    '200732/extrinsics/0',
    '214264/extrinsics/0',
    '214576/extrinsics/0',
    '243601/extrinsics/0',
    '244358/extrinsics/0',
    '287352/extrinsics/0',
    '300532/extrinsics/0',
    '301569/extrinsics/0',
    '302396/extrinsics/0',
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
