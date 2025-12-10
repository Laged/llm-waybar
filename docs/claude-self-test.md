# Claude Self-Test for waybar-llm-bridge

This document is a runnable test that Claude can execute on itself to verify waybar-llm-bridge is working correctly.

## Prerequisites

The binary must be built and accessible:
```bash
nix build
```

## Test Protocol

### Phase 1: Baseline State Check

First, check the current state file:

```bash
./result/bin/waybar-llm-bridge status
```

Expected: JSON output with current state (may show "idle" or previous state).

Also check if hooks are installed:
```bash
./result/bin/waybar-llm-bridge install-hooks --dry-run
```

### Phase 2: Start State Monitor

In a separate terminal (or background), watch the state file for changes:

```bash
watch -n 0.5 'cat /run/user/$(id -u)/llm_state.json 2>/dev/null || echo "No state file yet"'
```

### Phase 3: Generate Activity

Claude should now perform work that triggers hooks. Execute these steps with 3-second pauses between each to allow observation:

**Step 3.1**: Read a file (triggers tool hooks)
```
Read the file ./Cargo.toml
```

**Step 3.2**: Search for something (triggers tool hooks)
```
Grep for "waybar" in the codebase
```

**Step 3.3**: Write a simple 10-item TODO list to generate ~100 tokens:

```
1. Verify state file exists at /run/user/$UID/llm_state.json
2. Check that hooks fire on UserPromptSubmit
3. Confirm PreToolUse updates state to "tool-active"
4. Verify PostToolUse returns state to "thinking"
5. Test that Stop event sets state to "idle"
6. Validate JSON format matches Waybar expectations
7. Ensure atomic writes prevent partial reads
8. Check signal delivery to Waybar process
9. Test statusline command with piped JSON
10. Verify install-hooks merges without duplicates
```

### Phase 4: Final State Check

After generating activity, verify the state was updated:

```bash
./result/bin/waybar-llm-bridge status
```

The state should show:
- `class`: Should have changed during tool use
- `text`: Should reflect recent activity
- `tooltip`: Should show session info if statusline was configured

## Automated Headless Test Script

Save this as `test-hooks.sh` and run it while Claude is active:

```bash
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
```

## Expected State Transitions

| Event | text | class |
|-------|------|-------|
| Submit | "Thinking..." | "thinking" |
| ToolStart (Read) | "Read" | "tool-active" |
| ToolEnd | "Thinking..." | "thinking" |
| Stop | "Idle" | "idle" |

## Verifying Waybar Integration

If Waybar is running with the custom/llm module configured, you should see:

1. The module icon/text change when Claude starts thinking
2. Different styling when tools are being used
3. Return to idle state when Claude stops

Waybar config for reference:
```json
"custom/llm": {
    "format": "{}",
    "return-type": "json",
    "exec": "cat /run/user/1000/llm_state.json",
    "interval": "once",
    "signal": 8
}
```
