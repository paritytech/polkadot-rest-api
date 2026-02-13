-- Runtime metadata version endpoint benchmark script
-- Tests the /v1/runtime/metadata/{version} endpoint for latency and throughput

local util = require("util")

local versions = {
    'v14',
    'v15',
}

local counter = 1

request = function()
    local version = versions[counter]
    counter = counter + 1
    if counter > #versions then
        counter = 1
    end
    return wrk.format("GET", "/v1/runtime/metadata/" .. version)
end

done = util.done()
