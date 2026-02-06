-- Paras crowdloans endpoint benchmark script
-- Tests the /v1/paras/crowdloans endpoint for latency and throughput
-- Aligned with Sidecar benchmark parameters

local util = require("util")

-- Historical blocks (matching Sidecar)
local blocks = {
    '8500000',
    '8750000',
    '9000000',
    '9250000',
    '9500000',
    '9750000',
    '10000000',
    '10250000',
    '10500000',
    '10557215',
    '10750000',
    '10868888',
    '11000000',
    '11250000',
}

local counter = 1

request = function()
    local block = blocks[counter]
    counter = counter + 1
    if counter > #blocks then
        counter = 1
    end
    return wrk.format("GET", "/v1/paras/crowdloans?at=" .. block)
end

done = util.done()
