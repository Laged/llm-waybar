#!/usr/bin/env bash
# Single session demo - shows tool activity progression

set -e
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/../lib.sh"

check_binary
clean_state

TOTAL=8
SID="demo-single"

echo -e "\n${YELLOW}═══ Single Session Demo ═══${NC}\n"

step 1 $TOTAL "Starting Claude session..."
run_and_sync "$BIN event --type stop --session-id $SID"
show_state
pace

step 2 $TOTAL "User submits prompt..."
run_and_sync "$BIN event --type submit --session-id $SID"
show_state
assert_state "activity" "Thinking"
pace

step 3 $TOTAL "Claude reads file..."
run_and_sync "$BIN event --type tool-start --tool Read --session-id $SID"
show_state
assert_state "activity" "Read"
assert_state_eq "class" "tool-active"
pace

step 4 $TOTAL "Claude edits file..."
run_and_sync "$BIN event --type tool-start --tool Edit --session-id $SID"
show_state
assert_state "activity" "Edit"
pace

step 5 $TOTAL "Claude runs command..."
run_and_sync "$BIN event --type tool-start --tool Bash --session-id $SID"
show_state
assert_state "activity" "Bash"
pace

step 6 $TOTAL "Claude thinking..."
run_and_sync "$BIN event --type tool-end --session-id $SID"
show_state
assert_state "activity" "Thinking"
pace

step 7 $TOTAL "Session complete..."
run_and_sync "$BIN event --type stop --session-id $SID"
show_state
assert_state "activity" "Idle"
assert_state_eq "class" "idle"
pace

step 8 $TOTAL "All state transitions verified"
echo -e "\n${GREEN}✓ Demo complete!${NC}\n"
