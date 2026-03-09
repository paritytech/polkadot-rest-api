-- Pallets staking progress endpoint benchmark script
-- Tests the /v1/pallets/staking/progress endpoint for latency and throughput
-- Aligned with Sidecar benchmark parameters

local util = require("util")

-- Historical blocks (matching Sidecar)
local blocks = {
    '11000000',
    '10500000',
    '10000000',
    '9500000',
    '9000000',
    '8500000',
    '8000000',
    '7500000',
    '7000000',
    '6500000',
    '6000000',
    '5500000',
    '5000000',
    '4500000',
    '4000000',
    '3500000',
    '3000000',
    '2000000',
    '1000000',
}

local counter = 1

request = function()
    local block = blocks[counter]
    counter = counter + 1
    if counter > #blocks then
        counter = 1
    end
    return wrk.format("GET", "/v1/pallets/staking/progress?at=" .. block)
end

done = util.done()
