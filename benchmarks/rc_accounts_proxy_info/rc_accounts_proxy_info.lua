-- RC accounts proxy-info endpoint benchmark script
-- Tests the /v1/rc/accounts/{accountId}/proxy-info endpoint for latency and throughput
-- Note: This endpoint is only available on parachains

local util = require("util")

local endpoints = {
    '1KvKReVmUiTc2LW2a4qyHsaJJ9eE9LRsywZkMk5hyBeyHgw/proxy-info',
    '14Kq2Gt4buLr8XgRQmLtbWLHkejmhvGhiZDqLEzWcbe7jQTU/proxy-info',
    '15kUt2i86LHRWCkE3D9Bg1HZAoc2smhn1fwPzDERTb1BXAkX/proxy-info',
    '13BN4WksoyexwDWhGsMMUbU5okehD19GzdyqL4DMPR2KkQpP/proxy-info',
    '16MNMABGfPChG1RHxeb2YzoWUrX22G5CPnvarkmDJXzsZVRV/proxy-info',
    '13KJ3t8w1CKMkXCmZ6s3VwdWo4h747kXE88ZNh6rCBTvojmM/proxy-info',
    '12HFymxpDmi4XXPHaEMp74CNpRhkqwG5qxnrgikkhon1XMrj/proxy-info',
    '15GADXLmZpfCDgVcPuLGCwLAWw3hV9UpwPHw9BJuZEkQREqB/proxy-info',
    '148fP7zCq1JErXCy92PkNam4KZNcroG9zbbiPwMB1qehgeT4/proxy-info',
    '121bKwxHucGnDavnkQymq2hW12hsQ3KvXR1zJwAWiafG3Lfx/proxy-info',
}

local counter = 1

request = function()
    local endpoint = endpoints[counter]
    counter = counter + 1
    if counter > #endpoints then
        counter = 1
    end
    return wrk.format("GET", "/v1/rc/accounts/" .. endpoint)
end

done = util.done()
