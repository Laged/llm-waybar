# Demo System & Multi-Session Aggregation Design

**Created:** 2025-12-12
**Status:** Approved
**Priority:** High

## Overview

Add a visual demo/test system and multi-session support to llm-waybar. The demo serves dual purposes: automated integration testing AND visual showcase for README/marketing.

## Requirements

| Requirement | Description |
|-------------|-------------|
| **Demo purpose** | Test suite that doubles as visual demo |
| **Multi-session** | Aggregate view showing all active sessions |
| **Test approach** | Hybrid: fast simulated unit tests + real Claude integration tests |
| **Visual output** | Terminal showing test progress, synced with waybar updates |
| **Recording** | User handles recording; tests provide paced/interactive modes |

## Architecture

### Multi-Session State Files

Each Claude session writes to its own state file:

```
/run/user/$UID/llm_sessions/
├── abc123.json      # Session in ~/project-a
├── def456.json      # Session in ~/project-b
└── ghi789.json      # Session in ~/project-c
```

Session file schema (same as current WaybarState + metadata):
```json
{
  "session_id": "abc123",
  "cwd": "/home/user/project-a",
  "model": "Opus 4.5",
  "activity": "Thinking",
  "cost": 2.50,
  "input_tokens": 12000,
  "output_tokens": 3000,
  "cache_read": 45000,
  "cache_write": 2000,
  "last_activity_time": 1734012345,
  "class": "thinking",
  "alt": "active"
}
```

### Daemon Aggregator

New command: `waybar-llm-bridge daemon --aggregate`

**Responsibilities:**
1. Watch `/run/user/$UID/llm_sessions/` directory using `inotify`
2. On any file change, read all session files
3. Compute aggregate state:
   - `text`: Icon summary (e.g., "2 󰔟 1 󰈔" for 2 thinking, 1 reading)
   - `tooltip`: Per-session breakdown with project paths
   - `class`: "active" if any session active, "idle" if all idle
   - `cost`: Sum of all session costs
4. Write aggregate to `/run/user/$UID/llm_state.json`
5. Signal waybar (SIGRTMIN+8)

**Session lifecycle:**
- `statusline` hook creates/updates session file using `session_id`
- `stop` event marks session as idle
- Daemon removes stale sessions (no update for 5 minutes)

### Aggregate State Format

```json
{
  "text": "2 󰔟 1 󰈔",
  "tooltip": "3 active sessions | $5.23 total\n\n~/project-a: Opus 4.5 - Thinking ($2.50)\n~/project-b: Sonnet 3.5 - Read ($1.73)\n~/project-c: Opus 4.5 - Idle ($1.00)",
  "class": "active",
  "alt": "active",
  "percentage": 0,
  "sessions": 3,
  "total_cost": 5.23
}
```

## Test Suite Architecture

### Nix Flake Integration

```nix
{
  checks.x86_64-linux = {
    unit-tests = cargoTest;           # Fast: ~5 seconds
    integration-tests = claudeTests;   # Slow: requires Claude
  };

  apps.x86_64-linux = {
    demo = demoScript;                 # Visual demo runner
  };
}
```

### Unit Tests (Simulated, Fast)

Location: `tests/integration/`

```bash
nix flake check  # Runs in CI
```

Tests:
- Event hooks update state correctly
- Statusline preserves activity
- Format string placeholders work
- Icon mapping is correct
- Activity timeout resets to idle
- Multi-session aggregation computes correct summary

### Integration Tests (Real Claude, Optional)

```bash
nix flake check -- --integration  # Requires ANTHROPIC_API_KEY
```

Tests:
- Spawn headless Claude: `claude --dangerously-skip-permissions -p "Read README.md"`
- Verify hooks fire and state file updates
- Full round-trip validation

### Demo Script

Location: `demo/demo.sh`

```bash
# Modes
nix run .#demo                    # Normal speed
nix run .#demo -- --pace 2        # 2 seconds between actions
nix run .#demo -- --interactive   # Press Enter for each step
```

## Demo Scenarios

### Scenario 1: Single Session (`demo/scenarios/single-session.sh`)

```
[1/8] Starting Claude session...
      State: 󰒲 Idle | $0.00

[2/8] User submits prompt...
      State: 󰔟 Thinking | $0.00

[3/8] Claude reads file...
      State: 󰈔 Read | $0.02

[4/8] Claude edits file...
      State: 󰏫 Edit | $0.05

[5/8] Claude runs command...
      State: 󰆍 Bash | $0.08

[6/8] Claude thinking...
      State: 󰔟 Thinking | $0.12

[7/8] Session complete...
      State: 󰒲 Idle | $0.15

[8/8] ✓ All state transitions verified
```

### Scenario 2: Multi-Session (`demo/scenarios/multi-session.sh`)

```
[1/6] Starting session A in ~/project-a...
      State: 󰔟 Thinking | $0.00

[2/6] Starting session B in ~/project-b...
      State: 2 󰔟 | $0.00 (2 thinking)

[3/6] Session A reads file...
      State: 1 󰔟 1 󰈔 | $0.03

[4/6] Session B completes...
      State: 1 󰈔 | $0.05

[5/6] Session A completes...
      State: 󰒲 Idle | $0.08

[6/6] ✓ Aggregation verified
```

### Scenario 3: Cost Tracking (`demo/scenarios/cost-tracking.sh`)

Shows cost accumulating across tool usage with tooltip breakdown.

## File Structure

```
llm-waybar/
├── flake.nix                        # Add checks + demo app
├── demo/
│   ├── demo.sh                      # Main demo runner
│   ├── lib.sh                       # Helpers (colors, sync, assertions)
│   └── scenarios/
│       ├── single-session.sh        # Basic tool usage
│       ├── multi-session.sh         # Aggregate view
│       └── cost-tracking.sh         # Cost updates
├── tests/
│   └── integration/
│       ├── test-events.sh           # Event hook tests
│       ├── test-statusline.sh       # Statusline tests
│       └── test-aggregation.sh      # Multi-session tests
└── crates/waybar-llm-bridge/src/
    ├── main.rs                      # Add --aggregate flag to daemon
    └── aggregator.rs                # New: aggregation logic
```

## Implementation Changes

### 1. Session-Specific State Files

Modify `handle_statusline()` and `handle_event()` to:
- Extract `session_id` from statusline JSON (already available)
- Write to `/run/user/$UID/llm_sessions/{session_id}.json`
- Backward compatible: also write to legacy single-file location

### 2. Aggregator Module (`aggregator.rs`)

```rust
pub struct SessionAggregator {
    sessions_dir: PathBuf,
    output_path: PathBuf,
    signal: u8,
}

impl SessionAggregator {
    pub fn watch(&self) -> Result<()>;           // inotify loop
    pub fn aggregate(&self) -> AggregateState;   // Read all, compute summary
    fn cleanup_stale(&self);                     // Remove old sessions
}

pub struct AggregateState {
    pub text: String,        // "2 󰔟 1 󰈔"
    pub tooltip: String,     // Per-session breakdown
    pub class: String,       // "active" or "idle"
    pub sessions: usize,     // Count
    pub total_cost: f64,     // Sum
}
```

### 3. CLI Changes

```rust
enum Commands {
    // ... existing commands ...

    Daemon {
        #[arg(long)]
        log_path: Option<PathBuf>,  // Existing: watch transcript

        #[arg(long)]
        aggregate: bool,             // New: aggregate mode

        #[arg(long, default_value = "/run/user/$UID/llm_sessions")]
        sessions_dir: Option<PathBuf>,
    },
}
```

### 4. Demo Library (`demo/lib.sh`)

```bash
# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
CYAN='\033[0;36m'
NC='\033[0m'

# Run command and wait for waybar sync
run_and_sync() {
    local cmd="$1"
    eval "$cmd"
    sleep 0.1  # Signal propagation
}

# Assert state contains expected value
assert_state() {
    local field="$1"
    local expected="$2"
    local actual=$(cat "$STATE_FILE" | jq -r ".$field")
    if [[ "$actual" == *"$expected"* ]]; then
        echo -e "  ${GREEN}✓${NC} $field contains '$expected'"
        return 0
    else
        echo -e "  ${RED}✗${NC} $field: expected '$expected', got '$actual'"
        return 1
    fi
}

# Print current state nicely
show_state() {
    local text=$(cat "$STATE_FILE" | jq -r '.text')
    echo -e "      State: ${CYAN}$text${NC}"
}

# Pacing control
pace() {
    if [[ -n "$DEMO_PACE" ]]; then
        sleep "$DEMO_PACE"
    elif [[ "$DEMO_INTERACTIVE" == "1" ]]; then
        read -p "  Press Enter for next step..."
    fi
}
```

## Waybar Configuration Update

For aggregate mode:
```json
{
  "custom/llm": {
    "exec": "waybar-llm-bridge status",
    "return-type": "json",
    "interval": "once",
    "signal": 8
  }
}
```

Start daemon in your session startup:
```bash
waybar-llm-bridge daemon --aggregate &
```

## Migration Path

1. **Phase 1:** Add session-specific files + aggregator (backward compatible)
2. **Phase 2:** Add demo/test infrastructure
3. **Phase 3:** Update documentation and README with demo GIF

## Success Criteria

- [ ] `nix flake check` passes with unit tests
- [ ] `nix run .#demo` shows visual progress synced with waybar
- [ ] Multiple Claude sessions show aggregate state
- [ ] Demo can run at configurable pace for recording
- [ ] Stale sessions cleaned up automatically
