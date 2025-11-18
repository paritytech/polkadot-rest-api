-- Utility functions for wrk Lua scripts
local util = {}

-- Create a request function for a given endpoint
function util.request(handler, path)
    return function()
        return handler(path)
    end
end

-- Default delay function (no delay)
function util.delay()
    return function()
        -- No delay by default
    end
end

-- Signal that setup is complete and print statistics
function util.done()
    return function(summary, latency, requests)
        local bytes = summary.bytes
        local errors = summary.errors.status -- http status is not at the beginning of 200,300
        local total_requests = summary.requests -- total requests

        print("--------------------------")
        print("Total completed requests:       ", summary.requests)
        print("Failed requests:                ", summary.errors.status)
        print("Timeouts:                       ", summary.errors.connect or 0)
        print("Avg RequestTime(Latency):          "..string.format("%.2f",latency.mean / 1000).."ms")
        print("Max RequestTime(Latency):          "..(latency.max / 1000).."ms")
        print("Min RequestTime(Latency):          "..(latency.min / 1000).."ms")
        print("Benchmark finished.")
    end
end

return util
