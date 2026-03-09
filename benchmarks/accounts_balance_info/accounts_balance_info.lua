-- Accounts balance-info endpoint benchmark script
-- Tests the /v1/accounts/{accountId}/balance-info endpoint for latency and throughput
-- Aligned with Sidecar benchmark parameters
--
-- Chain-aware: adds Polkadot-specific endpoints when connected to Polkadot.

local util = require("util")

local chain = os.getenv("BENCH_CHAIN") or "polkadot"

-- Multiple accounts with historical blocks (matching Sidecar)
local endpoints = {
    '1KvKReVmUiTc2LW2a4qyHsaJJ9eE9LRsywZkMk5hyBeyHgw/balance-info?at=20000',
    '1KvKReVmUiTc2LW2a4qyHsaJJ9eE9LRsywZkMk5hyBeyHgw/balance-info?at=198702',
    '14Kq2Gt4buLr8XgRQmLtbWLHkejmhvGhiZDqLEzWcbe7jQTU/balance-info?at=2282256',
    '15kUt2i86LHRWCkE3D9Bg1HZAoc2smhn1fwPzDERTb1BXAkX/balance-info?at=3574738',
    '15kUt2i86LHRWCkE3D9Bg1HZAoc2smhn1fwPzDERTb1BXAkX/balance-info?at=4574738',
    '15kUt2i86LHRWCkE3D9Bg1HZAoc2smhn1fwPzDERTb1BXAkX/balance-info?at=6574738',
    '15kUt2i86LHRWCkE3D9Bg1HZAoc2smhn1fwPzDERTb1BXAkX/balance-info?at=7241122',
    '15kUt2i86LHRWCkE3D9Bg1HZAoc2smhn1fwPzDERTb1BXAkX/balance-info?at=8000000',
    '15kUt2i86LHRWCkE3D9Bg1HZAoc2smhn1fwPzDERTb1BXAkX/balance-info?at=8320000',
    '15kUt2i86LHRWCkE3D9Bg1HZAoc2smhn1fwPzDERTb1BXAkX/balance-info?at=8500000',
    '15kUt2i86LHRWCkE3D9Bg1HZAoc2smhn1fwPzDERTb1BXAkX/balance-info?at=9000000',
    '15kUt2i86LHRWCkE3D9Bg1HZAoc2smhn1fwPzDERTb1BXAkX/balance-info?at=9500000',
}

-- Polkadot-specific endpoints with newer blocks
if chain == "polkadot" then
    local polkadot_endpoints = {
        '13YMK2efcJncYrXsaJCvHbaaDt3vfubdn75r4hdVxcggU4n2/balance-info?at=19500000',
        '13fkJhLhs5cNCZ1GDRtwQifDnTS3BAW3b6SfmwJjThyFh9SH/balance-info?at=21500200',
        '16Drp38QW5UXWMHT7n5d5mPPH1u5Qavuv6aYAhbHfN3nzToe/balance-info?at=23800500',
        '12rgGkphjoZ25FubPoxywaNm3oVhSHnzExnT6hsLnicuLaaj/balance-info?at=24200500',
        '12KHAurRWMFJyxU57S9pQerHsKLCwvWKM1d3dKZVx7gSfkFJ/balance-info?at=25100300',
    }
    for _, ep in ipairs(polkadot_endpoints) do
        endpoints[#endpoints + 1] = ep
    end
end

util.print_endpoints(endpoints)

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
