-- Paras crowdloan-info endpoint benchmark script
-- Tests the /v1/paras/{paraId}/crowdloan-info endpoint for latency and throughput
-- Aligned with Sidecar benchmark parameters

local util = require("util")

-- Multiple paraIds with historical blocks (matching Sidecar)
local endpoints = {
    '2028/crowdloan-info?at=8500000',
    '2028/crowdloan-info?at=8750000',
    '2028/crowdloan-info?at=9000000',
    '2038/crowdloan-info?at=9250000',
    '2038/crowdloan-info?at=9500000',
    '2038/crowdloan-info?at=9750000',
    '2040/crowdloan-info?at=10000000',
    '2040/crowdloan-info?at=10250000',
    '2040/crowdloan-info?at=10500000',
    '2035/crowdloan-info?at=11000000',
    '2035/crowdloan-info?at=11250000',
    '2035/crowdloan-info?at=11500000',
    '2035/crowdloan-info?at=11750000',
    '2021/crowdloan-info?at=12000000',
}

local counter = 1

request = function()
    local endpoint = endpoints[counter]
    counter = counter + 1
    if counter > #endpoints then
        counter = 1
    end
    return wrk.format("GET", "/v1/paras/" .. endpoint)
end

done = util.done()
