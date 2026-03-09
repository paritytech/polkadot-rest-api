-- Pallets assets asset-info endpoint benchmark script
-- Tests the /v1/pallets/assets/{assetId}/asset-info endpoint for latency and throughput
-- Note: This endpoint is specific to Asset Hub chains

local util = require("util")

local asset_ids = {
    '1',
    '2',
    '3',
    '4',
    '5',
    '10',
    '100',
    '1000',
    '1984',
    '1337',
}

local counter = 1

request = function()
    local asset_id = asset_ids[counter]
    counter = counter + 1
    if counter > #asset_ids then
        counter = 1
    end
    return wrk.format("GET", "/v1/pallets/assets/" .. asset_id .. "/asset-info")
end

done = util.done()
