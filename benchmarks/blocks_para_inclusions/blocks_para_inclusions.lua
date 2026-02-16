-- Blocks para-inclusions endpoint benchmark script
-- Tests the /v1/blocks/{blockId}/para-inclusions endpoint for latency and throughput

local util = require("util")

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
    '10750000',
    '11000000',
    '11250000',
    '11500000',
    '11750000',
}

local counter = 1

request = function()
    local block = blocks[counter]
    counter = counter + 1
    if counter > #blocks then
        counter = 1
    end
    return wrk.format("GET", "/v1/blocks/" .. block .. "/para-inclusions")
end

done = util.done()
