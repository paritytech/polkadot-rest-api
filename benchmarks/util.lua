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

-- Signal that setup is complete
function util.done()
    return function()
        -- Setup complete
    end
end

return util
