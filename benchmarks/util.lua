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

-- Print the list of endpoints that will be tested (once across all wrk threads)
-- Uses a fixed temp file as a lock since each wrk thread has its own Lua state
function util.print_endpoints(endpoints)
    local lockfile = "/tmp/_wrk_bench_endpoints_printed"
    local f = io.open(lockfile, "r")
    if f then
        f:close()
        return
    end
    f = io.open(lockfile, "w")
    if f then f:close() end
    print("")
    print("Endpoints to benchmark (" .. #endpoints .. "):")
    for i, ep in ipairs(endpoints) do
        print("  " .. i .. ". " .. ep)
    end
    print("")
end

-- Signal that setup is complete and print statistics
-- Uses report.lua to emit JSON to stderr (captured by run.sh)
-- and prints human-readable summary to stdout
function util.done()
    local report = require("report")
    return report.done()
end

return util
