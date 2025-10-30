-- Version endpoint benchmark script
-- Tests the /version endpoint for latency and throughput

-- Setup the request
request = function()
    return wrk.format("GET", "/v1/version")
end

-- No delay between requests for maximum throughput
delay = function()
    -- No delay by default
end

-- Signal completion
done = function()
    -- Setup complete
end

