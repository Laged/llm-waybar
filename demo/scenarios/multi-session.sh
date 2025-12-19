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

# Note: This demo requires the aggregator daemon running
# Start it in another terminal: waybar-llm-bridge daemon --aggregate

step 1 $TOTAL "Starting session A in ~/project-a..."
run_and_sync "$BIN event --type submit --session-id $SID_A" "$SID_A"
show_state
pace

step 2 $TOTAL "Starting session B in ~/project-b..."
run_and_sync "$BIN event --type submit --session-id $SID_B" "$SID_B"
show_state
echo "      (Should show: 2 󰔟 for 2 thinking sessions)"
pace

step 3 $TOTAL "Session A reads file..."
run_and_sync "$BIN event --type tool-start --tool Read --session-id $SID_A" "$SID_A"
show_state
echo "      (Should show: 1 󰔟 1 󰈔 for 1 thinking, 1 reading)"
pace

step 4 $TOTAL "Session B edits file..."
run_and_sync "$BIN event --type tool-start --tool Edit --session-id $SID_B" "$SID_B"
show_state
echo "      (Should show: 1 󰈔 1 󰏫 for 1 reading, 1 editing)"
pace

step 5 $TOTAL "Session A completes..."
run_and_sync "$BIN event --type stop --session-id $SID_A" "$SID_A"
show_state
pace

step 6 $TOTAL "Session B completes..."
run_and_sync "$BIN event --type stop --session-id $SID_B" "$SID_B"
show_state
assert_state "activity" "Idle"
pace

step 7 $TOTAL "Aggregation verified"
echo -e "\n${GREEN}✓ Multi-session demo complete!${NC}\n"
