-- Accounts asset-approvals endpoint benchmark script
-- Tests the /v1/accounts/{accountId}/asset-approvals endpoint for latency and throughput
-- Note: This endpoint is specific to Asset Hub chains

local util = require("util")

local endpoints = {
    '1KvKReVmUiTc2LW2a4qyHsaJJ9eE9LRsywZkMk5hyBeyHgw/asset-approvals',
    '14Kq2Gt4buLr8XgRQmLtbWLHkejmhvGhiZDqLEzWcbe7jQTU/asset-approvals',
    '15kUt2i86LHRWCkE3D9Bg1HZAoc2smhn1fwPzDERTb1BXAkX/asset-approvals',
    '13BN4WksoyexwDWhGsMMUbU5okehD19GzdyqL4DMPR2KkQpP/asset-approvals',
    '16MNMABGfPChG1RHxeb2YzoWUrX22G5CPnvarkmDJXzsZVRV/asset-approvals',
    '13KJ3t8w1CKMkXCmZ6s3VwdWo4h747kXE88ZNh6rCBTvojmM/asset-approvals',
    '12HFymxpDmi4XXPHaEMp74CNpRhkqwG5qxnrgikkhon1XMrj/asset-approvals',
    '15GADXLmZpfCDgVcPuLGCwLAWw3hV9UpwPHw9BJuZEkQREqB/asset-approvals',
    '148fP7zCq1JErXCy92PkNam4KZNcroG9zbbiPwMB1qehgeT4/asset-approvals',
    '121bKwxHucGnDavnkQymq2hW12hsQ3KvXR1zJwAWiafG3Lfx/asset-approvals',
}

local counter = 1

request = function()
    local endpoint = endpoints[counter]
    counter = counter + 1
    if counter > #endpoints then
        counter = 1
    end
    return wrk.format("GET", "/v1/accounts/" .. endpoint)
end

done = util.done()
