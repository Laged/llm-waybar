#!/usr/bin/env bash
# Multi-session demo - shows aggregate view

set -e
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib.sh"

check_binary
clean_state

TOTAL=7
SID_A="demo-project-a"
SID_B="demo-project-b"

echo -e "\n${YELLOW}═══ Multi-Session Demo ═══${NC}\n"
echo "This demo shows aggregated state from multiple Claude sessions."
echo

# Start the aggregator daemon in background
echo "Starting aggregator daemon..."
$BIN daemon --aggregate &
DAEMON_PID=$!
sleep 0.5  # Give daemon time to start

# Cleanup daemon on exit
cleanup_daemon() {
    if [[ -n "$DAEMON_PID" ]]; then
        kill $DAEMON_PID 2>/dev/null || true
        wait $DAEMON_PID 2>/dev/null || true
    fi
}
trap cleanup_daemon EXIT

# For multi-session, run event and let daemon aggregate, then read main state
multi_event() {
    local cmd="$1"
    eval "$cmd"
    sleep 0.3  # Give daemon time to aggregate
    # In live mode, capture aggregated state and immediately start holding
    if [[ "$DEMO_LIVE" == "1" ]]; then
        DEMO_HOLD_STATE=$(cat "$STATE_FILE" 2>/dev/null)
        # Immediately re-write a few times to claim the file
        for _ in {1..5}; do
            echo "$DEMO_HOLD_STATE" > "$STATE_FILE"
            signal_waybar
            sleep 0.02
        done
    else
        signal_waybar
    fi
}

step 1 $TOTAL "Starting session A in ~/project-a..."
multi_event "$BIN event --type submit --session-id $SID_A"
show_state
pace

step 2 $TOTAL "Starting session B in ~/project-b..."
multi_event "$BIN event --type submit --session-id $SID_B"
show_state
echo "      (Should show: 2 󰔟 for 2 thinking sessions)"
pace

step 3 $TOTAL "Session A reads file..."
multi_event "$BIN event --type tool-start --tool Read --session-id $SID_A"
show_state
echo "      (Should show: 1 󰔟 1 󰈔 for 1 thinking, 1 reading)"
pace

step 4 $TOTAL "Session B edits file..."
multi_event "$BIN event --type tool-start --tool Edit --session-id $SID_B"
show_state
echo "      (Should show: 1 󰈔 1 󰏫 for 1 reading, 1 editing)"
pace

step 5 $TOTAL "Session A completes..."
multi_event "$BIN event --type stop --session-id $SID_A"
show_state
pace

step 6 $TOTAL "Session B completes..."
multi_event "$BIN event --type stop --session-id $SID_B"
show_state
assert_state "activity" "Idle"
pace

step 7 $TOTAL "Aggregation verified"
echo -e "\n${GREEN}✓ Multi-session demo complete!${NC}\n"
