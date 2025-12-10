#!/usr/bin/env bash
set -e

BIN="./result/bin/waybar-llm-bridge"
STATE_FILE="${LLM_BRIDGE_STATE_PATH:-/run/user/$(id -u)/llm_state.json}"

echo "=== waybar-llm-bridge Self-Test ==="
echo "State file: $STATE_FILE"
echo

# Check binary exists
if [[ ! -x "$BIN" ]]; then
    echo "ERROR: Binary not found. Run 'nix build' first."
    exit 1
fi

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
