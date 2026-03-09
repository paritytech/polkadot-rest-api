-- Blocks header endpoint benchmark script
-- Tests the /v1/blocks/{blockId}/header endpoint for latency and throughput
-- Aligned with Sidecar benchmark parameters

local util = require("util")

-- Historical blocks (matching Sidecar)
local endpoints = {
    '28831/header',      -- Sudo setKey(0, -> 1)
    '29258/header',      -- sudo.sudo(forceTransfer)
    '188836/header',     -- sudo.sudoUncheckedWeight runtime upgrade(v5)
    '197681/header',     -- sudo.sudo(forceTransfer)
    '199405/header',     -- sudo.sudoUncheckedWeight runtime upgrade(v6)
    '200732/header',     -- sudo.sudo(batch assign indices)
    '214264/header',     -- sudo.sudoUncheckedWeight runtime upgrade(v7)
    '214576/header',     -- proxy sudo batch of transfers
    '243601/header',     -- proxy sudo batch of transfers
    '244358/header',     -- sudo.sudoUncheckedWeight runtime upgrade(v8)
    '287352/header',     -- sudo.sudo forceTransfer
    '300532/header',     -- proxy.addProxy for `Any` from sudo
    '301569/header',     -- proxy sudo mint claim
    '302396/header',     -- proxy sudo set vested claim
    '303079/header',     -- sudo.sudoUncheckedWeight runtime upgrade(v9)
    '304468/header',     -- proxy sudo set balance(W3F)(failed)
    '313396/header',     -- proxy sudo set storage
    '314201/header',     -- sudo.sudoUncheckedWeight runtime upgrade(v10)
    '314326/header',     -- proxy sudo set balance(W3F)
    '325148/header',     -- scheduler dispatched
    '326556/header',     -- sudo.sudo force new era always
    '341469/header',     -- proxy sudo force transfer
    '342400/header',     -- sudo.sudoUncheckedWeight runtime upgrade(v11)
    '342477/header',     -- sudo.sudo schedule regular validator set increases
    '442600/header',     -- scheduler dispatched
    '443963/header',     -- sudo.sudoUncheckedWeight runtime upgrade(v12)
    '444722/header',     -- proxy sudo batch of transfers
    '516904/header',     -- sudo.sudo batch of transfers
    '528470/header',     -- sudo.sudoUncheckedWeight runtime upgrade(v13)
    '543510/header',     -- sudo.sudo force transfer
    '645697/header',     -- proxy sudo batch of transfers
    '744556/header',     -- proxy sudo batch of transfers
    '746085/header',     -- sudo.sudoUncheckedWeight runtime upgrade(v15)
    '746605/header',     -- sudo.sudoAs add governance proxy
    '786421/header',     -- sudo force transfer
    '787923/header',     -- sudo.sudoUncheckedWeight runtime upgrade(v16)
    '790128/header',     -- proxy sudo batch of transfers
    '799302/header',     -- runtime upgraded no more sudo
    '799310/header',     -- after v17
    '943438/header',     -- democracy.vote
    '1603025/header',    -- staking.withdrawUnbonded
    '6800002/header',    -- blocks.transfer
    '11873016/header',   -- vesting.vest
}

local counter = 1

request = function()
    local endpoint = endpoints[counter]
    counter = counter + 1
    if counter > #endpoints then
        counter = 1
    end
    return wrk.format("GET", "/v1/blocks/" .. endpoint)
end

done = util.done()
