#!/bin/bash

# Script to test useRcBlock feature
# This script tests the useRcBlock functionality on all implemented endpoints
# and saves all results to a document file

set -e

BASE_URL="${BASE_URL:-http://localhost:8080/v1}"
COLOR_GREEN='\033[0;32m'
COLOR_RED='\033[0;31m'
COLOR_YELLOW='\033[1;33m'
COLOR_NC='\033[0m' # No Color

# Output file for results
OUTPUT_DIR="${OUTPUT_DIR:-./test_results}"
OUTPUT_FILE="${OUTPUT_FILE:-${OUTPUT_DIR}/useRcBlock_test_results_$(date +%Y%m%d_%H%M%S).md}"
mkdir -p "${OUTPUT_DIR}"

echo "================================================================"
echo "Testing useRcBlock Feature"
echo "================================================================"
echo ""
echo "Base URL: ${BASE_URL}"
echo "Output file: ${OUTPUT_FILE}"
echo ""

# Initialize output file
cat > "${OUTPUT_FILE}" << EOF
# useRcBlock Feature Test Results

**Test Date:** $(date -u +"%Y-%m-%d %H:%M:%S UTC")
**Base URL:** ${BASE_URL}
**Server:** $(curl -s "${BASE_URL}/health" 2>/dev/null | jq -r '.' || echo "Unknown")

---

EOF

# Check if server is running
echo "Checking if server is running..."
if ! curl -s -f "${BASE_URL}/health" > /dev/null 2>&1; then
    echo -e "${COLOR_RED}✗ Server is not running at ${BASE_URL}${COLOR_NC}"
    echo "Please start the server first:"
    echo "  export SAS_SUBSTRATE_URL=wss://rpc.polkadot.io"
    echo "  export SAS_SUBSTRATE_MULTI_CHAIN_URL='[{\"url\":\"wss://polkadot-asset-hub-rpc.polkadot.io\",\"type\":\"assethub\"}]'"
    echo "  cargo run --release --bin polkadot-rest-api"
    exit 1
fi
echo -e "${COLOR_GREEN}✓ Server is running${COLOR_NC}"
echo ""

# Test counter
TESTS_PASSED=0
TESTS_FAILED=0

# Function to test endpoint and save results
test_endpoint() {
    local name=$1
    local url=$2
    local should_be_array=$3
    local description=$4
    local method="${5:-GET}"
    
    echo "------------------------------------------------------------"
    echo "Test: ${name}"
    echo "URL: ${url}"
    if [ -n "$description" ]; then
        echo "Description: ${description}"
    fi
    echo ""
    
    # Extract query parameters for documentation
    local query_params=""
    if [[ "$url" == *"?"* ]]; then
        query_params="${url#*\?}"
    fi
    
    # Write test info to output file
    {
        echo "## Test: ${name}"
        echo ""
        echo "**Method:** ${method}"
        echo "**Endpoint:** ${url}"
        if [ -n "$query_params" ]; then
            echo "**Query Parameters:** \`${query_params}\`"
        fi
        if [ -n "$description" ]; then
            echo "**Description:** ${description}"
        fi
        echo ""
        echo "### Request"
        echo "\`\`\`"
        echo "curl -X ${method} \"${url}\""
        echo "\`\`\`"
        echo ""
        echo "### Response"
        echo ""
    } >> "${OUTPUT_FILE}"
    
    local response
    local start_time=$(date +%s.%N)
    response=$(curl -s -w "\n%{http_code}\n%{time_total}" "${url}" 2>/dev/null || echo -e "\n000\n0")
    local end_time=$(date +%s.%N)
    local http_code=$(echo "$response" | tail -n2 | head -n1)
    local time_total=$(echo "$response" | tail -n1)
    local body=$(echo "$response" | sed '$d' | sed '$d')
    
    # Write response metadata
    {
        echo "**HTTP Status:** ${http_code}"
        echo "**Response Time:** ${time_total}s"
        echo ""
    } >> "${OUTPUT_FILE}"
    
    if [ "$http_code" != "200" ]; then
        echo -e "${COLOR_RED}✗ Failed: HTTP ${http_code}${COLOR_NC}"
        echo "Response: ${body}"
        {
            echo "**Result:** ❌ Failed"
            echo ""
            echo "\`\`\`json"
            echo "${body}"
            echo "\`\`\`"
            echo ""
            echo "---"
            echo ""
        } >> "${OUTPUT_FILE}"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        echo ""
        return 1
    fi
    
    # Check if response is valid JSON
    if ! echo "$body" | jq . > /dev/null 2>&1; then
        echo -e "${COLOR_RED}✗ Failed: Invalid JSON response${COLOR_NC}"
        echo "Response: ${body:0:200}..."
        {
            echo "**Result:** ❌ Failed - Invalid JSON"
            echo ""
            echo "\`\`\`"
            echo "${body:0:500}"
            echo "\`\`\`"
            echo ""
            echo "---"
            echo ""
        } >> "${OUTPUT_FILE}"
        TESTS_FAILED=$((TESTS_FAILED + 1))
        echo ""
        return 1
    fi
    
    # Format JSON for output (pretty print, limit size for large responses)
    local formatted_json
    formatted_json=$(echo "$body" | jq . 2>/dev/null || echo "$body")
    local json_size=$(echo "$formatted_json" | wc -c)
    
    # Check if it's an array or object
    local json_type
    json_type=$(echo "$body" | jq -r 'type' 2>/dev/null || echo "unknown")
    
    if [ "$should_be_array" = "true" ]; then
        if [ "$json_type" = "array" ]; then
            local array_length
            array_length=$(echo "$body" | jq 'length' 2>/dev/null || echo "0")
            echo -e "${COLOR_GREEN}✓ Passed: Returns array with ${array_length} items${COLOR_NC}"
            
            # Write success result
            {
                echo "**Result:** ✅ Passed"
                echo "**Response Type:** Array"
                echo "**Array Length:** ${array_length}"
                echo ""
            } >> "${OUTPUT_FILE}"
            
            if [ "$array_length" -gt 0 ]; then
                echo "  First item structure:"
                echo "$body" | jq '.[0] | keys' 2>/dev/null || echo "  (unable to parse)"
                
                # Check for required fields
                local has_at has_data has_rc has_timestamp
                has_at=$(echo "$body" | jq '.[0] | has("at")' 2>/dev/null || echo "false")
                has_data=$(echo "$body" | jq '.[0] | has("data")' 2>/dev/null || echo "false")
                has_rc=$(echo "$body" | jq '.[0] | has("rcBlockNumber")' 2>/dev/null || echo "false")
                has_timestamp=$(echo "$body" | jq '.[0] | has("ahTimestamp")' 2>/dev/null || echo "false")
                
                {
                    echo "**First Item Structure:**"
                    echo "\`\`\`json"
                    echo "$body" | jq '.[0] | keys' 2>/dev/null || echo "[]"
                    echo "\`\`\`"
                    echo ""
                    echo "**Required Fields Check:**"
                    echo "- \`at\`: ${has_at}"
                    echo "- \`data\`: ${has_data}"
                    echo "- \`rcBlockNumber\`: ${has_rc}"
                    echo "- \`ahTimestamp\`: ${has_timestamp}"
                    echo ""
                } >> "${OUTPUT_FILE}"
                
                if [ "$has_at" = "true" ] && [ "$has_data" = "true" ] && [ "$has_rc" = "true" ] && [ "$has_timestamp" = "true" ]; then
                    echo -e "  ${COLOR_GREEN}✓ All required fields present (at, data, rcBlockNumber, ahTimestamp)${COLOR_NC}"
                else
                    echo -e "  ${COLOR_YELLOW}⚠ Missing some required fields${COLOR_NC}"
                fi
                
                # Write first item example (or full response if small)
                if [ "$json_size" -lt 50000 ]; then
                    {
                        echo "**Full Response:**"
                        echo "\`\`\`json"
                        echo "$formatted_json"
                        echo "\`\`\`"
                    } >> "${OUTPUT_FILE}"
                else
                    {
                        echo "**First Item Example:**"
                        echo "\`\`\`json"
                        echo "$body" | jq '.[0]' 2>/dev/null || echo "{}"
                        echo "\`\`\`"
                        echo ""
                        echo "*Note: Response is too large (${json_size} bytes). Showing first item only.*"
                    } >> "${OUTPUT_FILE}"
                fi
            else
                {
                    echo "**Response:** Empty array \`[]\`"
                    echo "\`\`\`json"
                    echo "[]"
                    echo "\`\`\`"
                } >> "${OUTPUT_FILE}"
            fi
            TESTS_PASSED=$((TESTS_PASSED + 1))
        else
            echo -e "${COLOR_RED}✗ Failed: Expected array, got ${json_type}${COLOR_NC}"
            echo "Response preview: ${body:0:200}..."
            {
                echo "**Result:** ❌ Failed - Expected array, got ${json_type}"
                echo ""
                echo "\`\`\`json"
                echo "$formatted_json" | head -50
                echo "\`\`\`"
            } >> "${OUTPUT_FILE}"
            TESTS_FAILED=$((TESTS_FAILED + 1))
        fi
    else
        if [ "$json_type" = "object" ]; then
            echo -e "${COLOR_GREEN}✓ Passed: Returns object (standard behavior)${COLOR_NC}"
            {
                echo "**Result:** ✅ Passed"
                echo "**Response Type:** Object"
                echo ""
                if [ "$json_size" -lt 50000 ]; then
                    echo "**Full Response:**"
                    echo "\`\`\`json"
                    echo "$formatted_json"
                    echo "\`\`\`"
                else
                    echo "**Response Preview:**"
                    echo "\`\`\`json"
                    echo "$formatted_json" | head -100
                    echo "..."
                    echo "\`\`\`"
                    echo ""
                    echo "*Note: Response is too large (${json_size} bytes). Showing preview only.*"
                fi
            } >> "${OUTPUT_FILE}"
            TESTS_PASSED=$((TESTS_PASSED + 1))
        else
            echo -e "${COLOR_RED}✗ Failed: Expected object, got ${json_type}${COLOR_NC}"
            echo "Response preview: ${body:0:200}..."
            {
                echo "**Result:** ❌ Failed - Expected object, got ${json_type}"
                echo ""
                echo "\`\`\`json"
                echo "$formatted_json" | head -50
                echo "\`\`\`"
            } >> "${OUTPUT_FILE}"
            TESTS_FAILED=$((TESTS_FAILED + 1))
        fi
    fi
    
    {
        echo ""
        echo "---"
        echo ""
    } >> "${OUTPUT_FILE}"
    
    echo ""
}

# Test 1: /blocks/head/header without useRcBlock (standard)
test_endpoint \
    "GET /blocks/head/header (standard)" \
    "${BASE_URL}/blocks/head/header" \
    "false" \
    "Should return single object"

# Test 2: /blocks/head/header with useRcBlock=true
test_endpoint \
    "GET /blocks/head/header?useRcBlock=true" \
    "${BASE_URL}/blocks/head/header?useRcBlock=true" \
    "true" \
    "Should return array of Asset Hub blocks"

# Test 3: /blocks/{blockId} without useRcBlock
test_endpoint \
    "GET /blocks/1000000 (standard)" \
    "${BASE_URL}/blocks/1000000" \
    "false" \
    "Should return single block object"

# Test 4: /blocks/{blockId} with useRcBlock=true
# Using a reasonable RC block number (around 20M for Polkadot)
test_endpoint \
    "GET /blocks/10554957?useRcBlock=true" \
    "${BASE_URL}/blocks/10554957?useRcBlock=true" \
    "true" \
    "Should return array of Asset Hub blocks for RC block 10554957"

# Test 5: /runtime/spec without useRcBlock
test_endpoint \
    "GET /runtime/spec (standard)" \
    "${BASE_URL}/runtime/spec" \
    "false" \
    "Should return single runtime spec object"

# Test 6: /runtime/spec with useRcBlock=true
test_endpoint \
    "GET /runtime/spec?at=10554957&useRcBlock=true" \
    "${BASE_URL}/runtime/spec?at=10554957&useRcBlock=true" \
    "true" \
    "Should return array of runtime specs for RC block 10554957"

# Test 7: Empty array case (very high block number)
test_endpoint \
    "GET /blocks/1?useRcBlock=true (edge case)" \
    "${BASE_URL}/blocks/1?useRcBlock=true" \
    "true" \
    "Edge case: Should return empty array [] when no Asset Hub blocks found for very low RC block number"

# Summary
echo "================================================================"
echo "Test Summary"
echo "================================================================"
echo -e "${COLOR_GREEN}Tests Passed: ${TESTS_PASSED}${COLOR_NC}"
echo -e "${COLOR_RED}Tests Failed: ${TESTS_FAILED}${COLOR_NC}"
echo ""
echo "Results saved to: ${OUTPUT_FILE}"
echo ""

# Write summary to output file
{
    echo "# Test Summary"
    echo ""
    echo "**Test Date:** $(date -u +"%Y-%m-%d %H:%M:%S UTC")"
    echo "**Total Tests:** $((TESTS_PASSED + TESTS_FAILED))"
    echo "**Tests Passed:** ${TESTS_PASSED} ✅"
    echo "**Tests Failed:** ${TESTS_FAILED} ❌"
    echo ""
    if [ $TESTS_FAILED -eq 0 ]; then
        echo "## ✅ All tests passed!"
    else
        echo "## ❌ Some tests failed"
    fi
    echo ""
    echo "---"
    echo ""
    echo "*Generated by test_use_rc_block.sh*"
} >> "${OUTPUT_FILE}"

if [ $TESTS_FAILED -eq 0 ]; then
    echo -e "${COLOR_GREEN}✓ All tests passed!${COLOR_NC}"
    echo ""
    echo "View full results: cat ${OUTPUT_FILE}"
    exit 0
else
    echo -e "${COLOR_RED}✗ Some tests failed${COLOR_NC}"
    echo ""
    echo "View full results: cat ${OUTPUT_FILE}"
    exit 1
fi

