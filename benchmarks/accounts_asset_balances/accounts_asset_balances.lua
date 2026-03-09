-- Accounts asset-balances endpoint benchmark script
-- Tests the /v1/accounts/{accountId}/asset-balances endpoint for latency and throughput
-- Note: This endpoint is specific to Asset Hub chains

local util = require("util")

local endpoints = {
    '1KvKReVmUiTc2LW2a4qyHsaJJ9eE9LRsywZkMk5hyBeyHgw/asset-balances',
    '14Kq2Gt4buLr8XgRQmLtbWLHkejmhvGhiZDqLEzWcbe7jQTU/asset-balances',
    '15kUt2i86LHRWCkE3D9Bg1HZAoc2smhn1fwPzDERTb1BXAkX/asset-balances',
    '13BN4WksoyexwDWhGsMMUbU5okehD19GzdyqL4DMPR2KkQpP/asset-balances',
    '16MNMABGfPChG1RHxeb2YzoWUrX22G5CPnvarkmDJXzsZVRV/asset-balances',
    '13KJ3t8w1CKMkXCmZ6s3VwdWo4h747kXE88ZNh6rCBTvojmM/asset-balances',
    '12HFymxpDmi4XXPHaEMp74CNpRhkqwG5qxnrgikkhon1XMrj/asset-balances',
    '15GADXLmZpfCDgVcPuLGCwLAWw3hV9UpwPHw9BJuZEkQREqB/asset-balances',
    '148fP7zCq1JErXCy92PkNam4KZNcroG9zbbiPwMB1qehgeT4/asset-balances',
    '121bKwxHucGnDavnkQymq2hW12hsQ3KvXR1zJwAWiafG3Lfx/asset-balances',
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
