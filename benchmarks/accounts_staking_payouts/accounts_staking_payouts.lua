-- Accounts staking-payouts endpoint benchmark script
-- Tests the /v1/accounts/{accountId}/staking-payouts endpoint for latency and throughput
-- Aligned with Sidecar benchmark parameters

local util = require("util")

-- Multiple validator accounts (matching Sidecar)
local endpoints = {
    '13BN4WksoyexwDWhGsMMUbU5okehD19GzdyqL4DMPR2KkQpP/staking-payouts',
    '19hc9w3yVhTgDZoW2YBsYYY4PRG2X7YeykzfyDukrebt5aF/staking-payouts',
    '16MNMABGfPChG1RHxeb2YzoWUrX22G5CPnvarkmDJXzsZVRV/staking-payouts',
    '14aZL4ujxRML7mrqNG6GGA2xz66L1HcecrdcXaR9f2XQKLr/staking-payouts',
    '1jScNH45VWA78Rp8Sz9pQTzTRmDpSQcYANUoTtH1EWRQCqD/staking-payouts',
    '13KJ3t8w1CKMkXCmZ6s3VwdWo4h747kXE88ZNh6rCBTvojmM/staking-payouts',
    '13arvDxeWcGWmh2hq3qB6GNwfNZULdAPKTf2wuaeMj9ZMJp9/staking-payouts',
    '12HFymxpDmi4XXPHaEMp74CNpRhkqwG5qxnrgikkhon1XMrj/staking-payouts',
    '14DZ3GPuvb8Z9z4UgxV1ikC7UoypLXWYDy77MjoQJ3qMByW2/staking-payouts',
    '16kDoP9nFg4KUkjb3SSNnkmibKs1spmakxy6JLVCAFxTeSa3/staking-payouts',
    '12pECvQp8dESMAYfQFV4A23aCcUyWWN6MftLizH2wxVxXZJW/staking-payouts',
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
