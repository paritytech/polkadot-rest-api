-- Blocks head header endpoint benchmark script
-- Tests the /blocks/head/header endpoint for latency and throughput

-- Setup the request
request = function()
    return wrk.format("GET", "/blocks/head/header")
end

-- No delay between requests for maximum throughput
delay = function()
    -- No delay by default
end

-- Signal completion
done = function()
    -- Setup complete
end

