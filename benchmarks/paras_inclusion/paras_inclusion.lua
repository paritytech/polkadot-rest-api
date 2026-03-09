-- Paras inclusion endpoint benchmark script
-- Tests the /v1/paras/{number}/inclusion endpoint for latency and throughput

local util = require("util")

-- Known parachain IDs on Polkadot
local para_ids = {
    '1000',
    '1001',
    '1002',
    '2000',
    '2004',
    '2006',
    '2011',
    '2012',
    '2030',
    '2034',
    '2035',
    '2043',
    '2046',
    '2048',
}

local counter = 1

request = function()
    local para_id = para_ids[counter]
    counter = counter + 1
    if counter > #para_ids then
        counter = 1
    end
    return wrk.format("GET", "/v1/paras/" .. para_id .. "/inclusion")
end

done = util.done()
