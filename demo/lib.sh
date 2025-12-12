#!/usr/bin/env bash
# Demo library functions

# Colors
export GREEN='\033[0;32m'
export RED='\033[0;31m'
export CYAN='\033[0;36m'
export YELLOW='\033[1;33m'
export NC='\033[0m' # No Color

# Paths
export BIN="${DEMO_BIN:-./result/bin/waybar-llm-bridge}"
export STATE_FILE="${LLM_BRIDGE_STATE_PATH:-/run/user/$(id -u)/llm_state.json}"
export SESSIONS_DIR="${LLM_BRIDGE_SESSIONS_DIR:-/run/user/$(id -u)/llm_sessions}"

# Run command and wait for waybar sync
run_and_sync() {
    eval "$1"
    sleep 0.15  # Signal propagation + file write
}

# Assert state field contains expected value
assert_state() {
    local field="$1"
    local expected="$2"
    local actual
    actual=$(jq -r ".$field" < "$STATE_FILE" 2>/dev/null || echo "")

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
    actual=$(jq -r ".$field" < "$STATE_FILE" 2>/dev/null || echo "")

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
    text=$(jq -r '.text' < "$STATE_FILE" 2>/dev/null || echo "no state")
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
        sleep "$DEMO_PACE"
    elif [[ "$DEMO_INTERACTIVE" == "1" ]]; then
        read -rp "  Press Enter for next step..."
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
