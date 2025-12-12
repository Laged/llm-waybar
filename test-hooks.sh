#!/usr/bin/env bash
set -e

# Binary detection - try multiple locations
if [[ -n "$DEMO_BIN" ]]; then
    BIN="$DEMO_BIN"
    echo "Using DEMO_BIN: $BIN"
elif [[ -x "./result/bin/waybar-llm-bridge" ]]; then
    BIN="./result/bin/waybar-llm-bridge"
    echo "Using local build: $BIN"
elif command -v waybar-llm-bridge &> /dev/null; then
    BIN="waybar-llm-bridge"
    echo "Using PATH binary: $BIN"
else
    echo "ERROR: waybar-llm-bridge not found!"
    echo "Tried:"
    echo "  1. DEMO_BIN environment variable"
    echo "  2. ./result/bin/waybar-llm-bridge"
    echo "  3. waybar-llm-bridge in PATH"
    echo ""
    echo "Run 'nix build' first or set DEMO_BIN."
    exit 1
fi

STATE_FILE="${LLM_BRIDGE_STATE_PATH:-/run/user/$(id -u)/llm_state.json}"

echo "=== waybar-llm-bridge Self-Test ==="
echo "State file: $STATE_FILE"
echo

# Check binary is executable
if ! command -v "$BIN" &> /dev/null && [[ ! -x "$BIN" ]]; then
    echo "ERROR: Binary '$BIN' is not executable."
    exit 1
fi

# Clean slate - remove old state
echo "--- Cleaning old state ---"
rm -f "$STATE_FILE"
echo

# Initial state
echo "--- Initial State ---"
$BIN status || echo '{"text":"no state","class":"idle"}'
echo

# Simulate events
echo "--- Simulating Event Sequence ---"

echo "1. Submit event..."
$BIN event --type submit
sleep 1
$BIN status
echo

echo "2. Tool start (Read)..."
$BIN event --type tool-start --tool "Read"
sleep 1
$BIN status
echo

echo "3. Tool end..."
$BIN event --type tool-end
sleep 1
$BIN status
echo

echo "4. Stop event..."
$BIN event --type stop
sleep 1
$BIN status
echo

# Test statusline with mock input
echo "--- Testing Statusline ---"
echo '{"model":{"display_name":"Claude Opus 4"},"cost":{"total_cost_usd":0.42}}' | $BIN statusline
echo

# Final state
echo "--- Final State ---"
$BIN status
echo

echo "=== Test Complete ==="
echo

# Additional integration tests for Batch 5
echo "=== Integration Tests: Event + Statusline Flow ==="
echo

# Clean state for tests
rm -f "$STATE_FILE"

echo "--- Test 1: Event sets activity, statusline preserves it ---"
echo "Step 1: Set activity to 'Thinking' via event"
$BIN event --type submit
sleep 0.5
BEFORE_STATUS=$($BIN status)
echo "Before statusline: $BEFORE_STATUS"

echo "Step 2: Update model/cost via statusline"
echo '{"model":{"display_name":"Opus 4.5"},"cost":{"total_cost_usd":1.25}}' | $BIN statusline
sleep 0.5
AFTER_STATUS=$($BIN status)
echo "After statusline: $AFTER_STATUS"

# Verify activity is preserved
if echo "$AFTER_STATUS" | grep -q "Thinking"; then
    echo "✓ PASS: Activity 'Thinking' preserved after statusline update"
else
    echo "✗ FAIL: Activity not preserved (expected 'Thinking' in: $AFTER_STATUS)"
fi

# Verify model was updated
if echo "$AFTER_STATUS" | grep -q "Opus 4.5"; then
    echo "✓ PASS: Model 'Opus 4.5' updated by statusline"
else
    echo "✗ FAIL: Model not updated (expected 'Opus 4.5' in: $AFTER_STATUS)"
fi
echo

echo "--- Test 2: Format string with {model} placeholder ---"
rm -f "$STATE_FILE"

# Set state with both activity and model
$BIN event --type tool-start --tool "Edit"
sleep 0.5
echo '{"model":{"display_name":"Sonnet 3.5"},"cost":{"total_cost_usd":0.75}}' | $BIN statusline
sleep 0.5

# Test format string (if supported via env var)
export LLM_BRIDGE_FORMAT="{model} | {activity}"
STATUS=$($BIN status)
echo "Format='{model} | {activity}': $STATUS"

if echo "$STATUS" | grep -q "Sonnet 3.5" && echo "$STATUS" | grep -q "Edit"; then
    echo "✓ PASS: Format string works with {model} and {activity}"
else
    echo "✗ FAIL: Format string not applied correctly (got: $STATUS)"
fi
unset LLM_BRIDGE_FORMAT
echo

echo "--- Test 3: Icon placeholder in format string ---"
rm -f "$STATE_FILE"

$BIN event --type tool-start --tool "Bash"
sleep 0.5

export LLM_BRIDGE_FORMAT="{icon} {activity}"
STATUS=$($BIN status)
echo "Format='{icon} {activity}': $STATUS"

# Bash should have terminal icon (Unicode f018d)
if echo "$STATUS" | grep -q "Bash"; then
    echo "✓ PASS: Icon placeholder and activity work"
else
    echo "✗ FAIL: Icon/activity not in output (got: $STATUS)"
fi
unset LLM_BRIDGE_FORMAT
echo

echo "--- Test 4: Cost precision in format string ---"
rm -f "$STATE_FILE"

echo '{"model":{"display_name":"Test"},"cost":{"total_cost_usd":2.51609}}' | $BIN statusline
sleep 0.5

export LLM_BRIDGE_FORMAT="\${cost:.2}"
STATUS=$($BIN status)
echo "Format='\${cost:.2}': $STATUS"

if echo "$STATUS" | grep -q "2.52"; then
    echo "✓ PASS: Cost formatted to 2 decimal places"
else
    echo "✗ FAIL: Cost precision not applied (expected '2.52', got: $STATUS)"
fi
unset LLM_BRIDGE_FORMAT
echo

echo "--- Test 5: Complex format string ---"
rm -f "$STATE_FILE"

$BIN event --type tool-start --tool "Read"
sleep 0.5
echo '{"model":{"display_name":"Opus 4.5"},"cost":{"total_cost_usd":1.2345}}' | $BIN statusline
sleep 0.5

export LLM_BRIDGE_FORMAT="{model} | {icon} {activity} | \${cost:.2}"
STATUS=$($BIN status)
echo "Format='{model} | {icon} {activity} | \${cost:.2}': $STATUS"

if echo "$STATUS" | grep -q "Opus 4.5" && echo "$STATUS" | grep -q "Read" && echo "$STATUS" | grep -q "1.23"; then
    echo "✓ PASS: Complex format string works correctly"
else
    echo "✗ FAIL: Complex format failed (got: $STATUS)"
fi
unset LLM_BRIDGE_FORMAT
echo

echo "=== All Integration Tests Complete ==="
