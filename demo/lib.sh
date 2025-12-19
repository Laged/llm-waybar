#!/usr/bin/env bash
# Demo library functions

# Colors
export GREEN='\033[0;32m'
export RED='\033[0;31m'
export CYAN='\033[0;36m'
export YELLOW='\033[1;33m'
export NC='\033[0m' # No Color

# Paths - use isolated paths by default to avoid conflicts with active sessions
export BIN="${DEMO_BIN:-./result/bin/waybar-llm-bridge}"

# Set paths based on --live flag
if [[ "$DEMO_LIVE" == "1" ]]; then
    # Use real waybar paths - will update actual waybar display
    export LLM_BRIDGE_STATE_PATH="${LLM_BRIDGE_STATE_PATH:-/run/user/$(id -u)/llm_state.json}"
    export LLM_BRIDGE_SESSIONS_DIR="${LLM_BRIDGE_SESSIONS_DIR:-/run/user/$(id -u)/llm_sessions}"
else
    # Use isolated demo paths
    export LLM_BRIDGE_STATE_PATH="${LLM_BRIDGE_STATE_PATH:-/tmp/llm_demo_state.json}"
    export LLM_BRIDGE_SESSIONS_DIR="${LLM_BRIDGE_SESSIONS_DIR:-/tmp/llm_demo_sessions}"
fi
export STATE_FILE="$LLM_BRIDGE_STATE_PATH"
export SESSIONS_DIR="$LLM_BRIDGE_SESSIONS_DIR"

# State to hold during pace (for --live mode)
DEMO_HOLD_STATE=""

# Run command and wait for waybar sync
# Usage: run_and_sync "command" [expected_session_id]
run_and_sync() {
    local cmd="$1"
    local expected_sid="$2"

    eval "$cmd"

    # In live mode, read from session file (not main state) to avoid race with other sessions
    if [[ "$DEMO_LIVE" == "1" && -n "$expected_sid" ]]; then
        local session_file="$SESSIONS_DIR/$expected_sid.json"
        if [[ -f "$session_file" ]]; then
            DEMO_HOLD_STATE=$(cat "$session_file" 2>/dev/null)
        else
            # Fallback to main state file
            DEMO_HOLD_STATE=$(cat "$STATE_FILE" 2>/dev/null)
        fi
        # Write to main state file for waybar to read
        echo "$DEMO_HOLD_STATE" > "$STATE_FILE"
    fi

    # Signal waybar to refresh (RTMIN+8 = signal 8 in waybar config)
    pkill -RTMIN+8 waybar 2>/dev/null || true
    sleep 0.05  # Signal propagation
}

# Helper to get state JSON (uses DEMO_HOLD_STATE in live mode)
_get_state_json() {
    if [[ "$DEMO_LIVE" == "1" && -n "$DEMO_HOLD_STATE" ]]; then
        echo "$DEMO_HOLD_STATE"
    else
        cat "$STATE_FILE" 2>/dev/null
    fi
}

# Assert state field contains expected value
assert_state() {
    local field="$1"
    local expected="$2"
    local actual
    actual=$(_get_state_json | jq -r ".$field" 2>/dev/null || echo "")

    if [[ "$actual" == *"$expected"* ]]; then
        echo -e "  ${GREEN}✓${NC} $field contains '$expected'"
        return 0
    else
        echo -e "  ${RED}✗${NC} $field: expected '$expected', got '$actual'"
        return 1
    fi
}

# Assert state field equals expected value exactly
assert_state_eq() {
    local field="$1"
    local expected="$2"
    local actual
    actual=$(_get_state_json | jq -r ".$field" 2>/dev/null || echo "")

    if [[ "$actual" == "$expected" ]]; then
        echo -e "  ${GREEN}✓${NC} $field == '$expected'"
        return 0
    else
        echo -e "  ${RED}✗${NC} $field: expected '$expected', got '$actual'"
        return 1
    fi
}

# Print current state nicely
show_state() {
    local text
    text=$(_get_state_json | jq -r '.text' 2>/dev/null || echo "no state")
    echo -e "      State: ${CYAN}$text${NC}"
}

# Show step header
step() {
    local num="$1"
    local total="$2"
    local desc="$3"
    echo -e "\n${YELLOW}[$num/$total]${NC} $desc"
}

# Pacing control
pace() {
    if [[ -n "$DEMO_PACE" ]]; then
        if [[ "$DEMO_LIVE" == "1" && -n "$DEMO_HOLD_STATE" ]]; then
            # In live mode, continuously re-enforce state to override other sessions
            local i=0
            local max_iter=$((DEMO_PACE * 5))  # ~5 iterations per second
            while [[ $i -lt $max_iter ]]; do
                sleep 0.2
                echo "$DEMO_HOLD_STATE" > "$STATE_FILE"
                pkill -RTMIN+8 waybar 2>/dev/null || true
                i=$((i + 1))  # Use $((i+1)) instead of ((i++)) to avoid set -e exit on 0
            done
        else
            sleep "$DEMO_PACE"
        fi
    elif [[ "$DEMO_INTERACTIVE" == "1" ]]; then
        if [[ "$DEMO_LIVE" == "1" && -n "$DEMO_HOLD_STATE" ]]; then
            # In interactive live mode, hold state in background while waiting
            (
                while true; do
                    sleep 0.3
                    echo "$DEMO_HOLD_STATE" > "$STATE_FILE"
                    pkill -RTMIN+8 waybar 2>/dev/null || true
                done
            ) &
            local hold_pid=$!
            read -rp "  Press Enter for next step..."
            kill $hold_pid 2>/dev/null || true
        else
            read -rp "  Press Enter for next step..."
        fi
    fi
}

# Clean session state
clean_state() {
    rm -f "$STATE_FILE"
    rm -rf "$SESSIONS_DIR"
    mkdir -p "$SESSIONS_DIR"
}

# Check binary exists
check_binary() {
    if [[ ! -x "$BIN" ]]; then
        echo -e "${RED}ERROR:${NC} Binary not found at $BIN"
        echo "Run 'nix build' first."
        exit 1
    fi
}
