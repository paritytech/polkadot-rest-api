-- RC blocks extrinsics-raw endpoint benchmark script
-- Tests the /v1/rc/blocks/{blockId}/extrinsics-raw endpoint for latency and throughput
-- Note: This endpoint is only available on parachains

local util = require("util")

local blocks = {
    '28831',
    '29258',
    '188836',
    '197681',
    '199405',
    '200732',
    '214264',
    '214576',
    '243601',
    '244358',
    '287352',
    '300532',
    '301569',
    '302396',
}

local counter = 1

request = function()
    local block = blocks[counter]
    counter = counter + 1
    if counter > #blocks then
        counter = 1
    end
    return wrk.format("GET", "/v1/rc/blocks/" .. block .. "/extrinsics-raw")
end

done = util.done()
