-- Coretime reservations endpoint benchmark script
-- Tests the /v1/coretime/reservations endpoint for latency and throughput
-- Note: This endpoint is specific to Coretime chains

local util = require("util")

request = function()
    return wrk.format("GET", "/v1/coretime/reservations")
end

done = util.done()
