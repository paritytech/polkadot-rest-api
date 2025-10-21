-- Health endpoint benchmark script
-- Tests the /health endpoint for latency and throughput

-- Setup the request
request = function()
    return wrk.format("GET", "/health")
end

-- No delay between requests for maximum throughput
delay = function()
    -- No delay by default
end

-- Signal completion
done = function()
    -- Setup complete
end
