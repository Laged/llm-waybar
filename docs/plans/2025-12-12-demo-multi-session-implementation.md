# Demo System & Multi-Session Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add multi-session aggregation and a visual demo/test system to llm-waybar.

**Architecture:** Session-specific state files written by hooks, daemon aggregator watches and combines them, demo scripts simulate activity with visual output synced to waybar.

**Tech Stack:** Rust (notify crate for inotify), Bash (demo scripts), Nix (flake checks/apps)

---

## Batch 1: Session State Infrastructure

**Checkpoint:** Sessions write to individual files, backward compatible with single-file mode

### Task 1.1: Add SessionState struct with session metadata

**Files:**
- Modify: `crates/llm-bridge-core/src/state.rs`

**Step 1: Write the failing test**

Add to the existing test module in `state.rs`:

```rust
#[test]
fn test_session_state_has_metadata() {
    let state = WaybarState::default();
    assert_eq!(state.session_id, "");
    assert_eq!(state.cwd, "");
}
```

**Step 2: Run test to verify it fails**

Run: `nix develop -c cargo test test_session_state_has_metadata`
Expected: FAIL with "no field `session_id`"

**Step 3: Add session metadata fields to WaybarState**

Add these fields after `last_activity_time` in `WaybarState` struct:

```rust
    #[serde(default)]
    pub session_id: String,          // Claude session ID
    #[serde(default)]
    pub cwd: String,                 // Working directory
```

Update `Default` impl to include:
```rust
    session_id: String::new(),
    cwd: String::new(),
```

**Step 4: Run test to verify it passes**

Run: `nix develop -c cargo test test_session_state_has_metadata`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/llm-bridge-core/src/state.rs
git commit -m "feat(state): add session_id and cwd fields for multi-session support"
```

---

### Task 1.2: Add sessions directory config

**Files:**
- Modify: `crates/llm-bridge-core/src/config.rs`

**Step 1: Add sessions_dir to Config struct**

```rust
#[derive(Debug, Clone)]
pub struct Config {
    pub state_path: PathBuf,
    pub signal: u8,
    pub transcript_dir: PathBuf,
    pub format: String,
    pub sessions_dir: PathBuf,  // NEW
}
```

**Step 2: Add default function**

```rust
fn default_sessions_dir() -> PathBuf {
    if let Ok(runtime_dir) = env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(runtime_dir).join("llm_sessions")
    } else {
        PathBuf::from("/tmp/llm_sessions")
    }
}
```

**Step 3: Update Default and from_env**

In `Default`:
```rust
    sessions_dir: default_sessions_dir(),
```

In `from_env`:
```rust
    sessions_dir: env::var("LLM_BRIDGE_SESSIONS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| default_sessions_dir()),
```

**Step 4: Verify it compiles**

Run: `nix develop -c cargo check -p llm-bridge-core`
Expected: Success

**Step 5: Commit**

```bash
git add crates/llm-bridge-core/src/config.rs
git commit -m "feat(config): add sessions_dir for multi-session state files"
```

---

### Task 1.3: Add session file write helper

**Files:**
- Modify: `crates/llm-bridge-core/src/state.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn test_write_session_file() {
    let dir = std::env::temp_dir().join("llm_test_sessions");
    std::fs::create_dir_all(&dir).unwrap();

    let mut state = WaybarState::default();
    state.session_id = "test123".to_string();
    state.activity = "Thinking".to_string();

    state.write_session_file(&dir).unwrap();

    let session_file = dir.join("test123.json");
    assert!(session_file.exists());

    let content = std::fs::read_to_string(&session_file).unwrap();
    assert!(content.contains("test123"));

    // Cleanup
    std::fs::remove_dir_all(&dir).ok();
}
```

**Step 2: Run test to verify it fails**

Run: `nix develop -c cargo test test_write_session_file`
Expected: FAIL with "no method named `write_session_file`"

**Step 3: Implement write_session_file**

Add to `impl WaybarState`:

```rust
    /// Write state to session-specific file in sessions directory
    pub fn write_session_file(&self, sessions_dir: &Path) -> std::io::Result<()> {
        if self.session_id.is_empty() {
            return Ok(()); // No session ID, skip session file
        }

        // Ensure directory exists
        fs::create_dir_all(sessions_dir)?;

        let session_path = sessions_dir.join(format!("{}.json", self.session_id));
        self.write_atomic(&session_path)
    }
```

**Step 4: Run test to verify it passes**

Run: `nix develop -c cargo test test_write_session_file`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/llm-bridge-core/src/state.rs
git commit -m "feat(state): add write_session_file for multi-session support"
```

---

### Task 1.4: Update handle_statusline to write session file

**Files:**
- Modify: `crates/waybar-llm-bridge/src/main.rs`

**Step 1: Update handle_statusline signature**

Add `sessions_dir` parameter:

```rust
fn handle_statusline(
    state_path: &PathBuf,
    sessions_dir: &PathBuf,  // NEW
    signal: u8,
    format: &str,
) -> Result<(), Box<dyn std::error::Error>> {
```

**Step 2: Store session_id and cwd in state**

After parsing `status_input`, add:

```rust
    // Store session metadata
    if let Some(ref sid) = status_input.session_id {
        state.session_id = sid.clone();
    }
    if let Some(ref cwd) = status_input.cwd {
        state.cwd = cwd.clone();
    }
```

**Step 3: Write to both locations**

Before the existing `state.write_atomic(state_path)?;`, add:

```rust
    // Write to session-specific file (for multi-session aggregation)
    let _ = state.write_session_file(sessions_dir);
```

**Step 4: Update call site in main()**

Update the `Commands::Statusline` arm:

```rust
    Commands::Statusline => {
        handle_statusline(&state_path, &config.sessions_dir, cli.signal, &format)
    }
```

**Step 5: Verify it compiles**

Run: `nix develop -c cargo build -p waybar-llm-bridge`
Expected: Success

**Step 6: Commit**

```bash
git add crates/waybar-llm-bridge/src/main.rs
git commit -m "feat(statusline): write session-specific state files"
```

---

### Task 1.5: Update handle_event to accept session_id

**Files:**
- Modify: `crates/waybar-llm-bridge/src/main.rs`

**Step 1: Add --session-id flag to Event command**

```rust
    Event {
        #[arg(long, value_enum)]
        r#type: EventType,
        #[arg(long)]
        tool: Option<String>,
        #[arg(long)]
        payload: Option<String>,
        #[arg(long)]
        session_id: Option<String>,  // NEW
    },
```

**Step 2: Update handle_event signature**

```rust
fn handle_event(
    event_type: EventType,
    tool: Option<String>,
    _payload: Option<String>,
    session_id: Option<String>,  // NEW
    state_path: &PathBuf,
    sessions_dir: &PathBuf,      // NEW
    signal: u8,
    format: &str,
) -> Result<(), Box<dyn std::error::Error>> {
```

**Step 3: Set session_id and write session file**

After updating activity fields, add:

```rust
    // Set session_id if provided
    if let Some(sid) = session_id {
        state.session_id = sid;
    }

    // Write to session-specific file
    let _ = state.write_session_file(sessions_dir);
```

**Step 4: Update call site**

```rust
    Commands::Event { r#type, tool, payload, session_id } => {
        handle_event(r#type, tool, payload, session_id, &state_path, &config.sessions_dir, cli.signal, &format)
    }
```

**Step 5: Verify it compiles**

Run: `nix develop -c cargo build -p waybar-llm-bridge`
Expected: Success

**Step 6: Commit**

```bash
git add crates/waybar-llm-bridge/src/main.rs
git commit -m "feat(event): add --session-id flag for multi-session support"
```

---

## Batch 2: Session Aggregator

**Checkpoint:** Daemon can watch sessions directory and aggregate state

### Task 2.1: Create aggregator module

**Files:**
- Create: `crates/waybar-llm-bridge/src/aggregator.rs`
- Modify: `crates/waybar-llm-bridge/src/main.rs` (add mod)

**Step 1: Create the aggregator module file**

```rust
//! Session aggregation for multi-session waybar display

use llm_bridge_core::{WaybarState, signal::signal_waybar};
use notify::{Watcher, RecursiveMode, Event, EventKind};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Aggregated state from multiple sessions
#[derive(Debug, Clone)]
pub struct AggregateState {
    pub text: String,
    pub tooltip: String,
    pub class: String,
    pub alt: String,
    pub sessions: usize,
    pub total_cost: f64,
}

impl Default for AggregateState {
    fn default() -> Self {
        Self {
            text: "Idle".to_string(),
            tooltip: String::new(),
            class: "idle".to_string(),
            alt: "idle".to_string(),
            sessions: 0,
            total_cost: 0.0,
        }
    }
}

/// Session aggregator that watches a directory of session files
pub struct SessionAggregator {
    sessions_dir: PathBuf,
    output_path: PathBuf,
    signal: u8,
    stale_timeout_secs: u64,
}

impl SessionAggregator {
    pub fn new(sessions_dir: PathBuf, output_path: PathBuf, signal: u8) -> Self {
        Self {
            sessions_dir,
            output_path,
            signal,
            stale_timeout_secs: 300, // 5 minutes
        }
    }

    /// Read all session files and compute aggregate state
    pub fn aggregate(&self) -> AggregateState {
        let mut sessions: Vec<WaybarState> = Vec::new();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        if let Ok(entries) = fs::read_dir(&self.sessions_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "json").unwrap_or(false) {
                    if let Ok(state) = WaybarState::read_from(&path) {
                        // Skip stale sessions
                        if state.last_activity_time > 0
                            && (now - state.last_activity_time) < self.stale_timeout_secs as i64
                        {
                            sessions.push(state);
                        }
                    }
                }
            }
        }

        self.compute_aggregate(&sessions)
    }

    fn compute_aggregate(&self, sessions: &[WaybarState]) -> AggregateState {
        if sessions.is_empty() {
            return AggregateState::default();
        }

        // Count activities by type
        let mut activity_counts: HashMap<String, usize> = HashMap::new();
        let mut total_cost = 0.0;
        let mut any_active = false;

        for session in sessions {
            let activity = &session.activity;
            *activity_counts.entry(activity.clone()).or_insert(0) += 1;
            total_cost += session.cost;
            if activity != "Idle" {
                any_active = true;
            }
        }

        // Build text with icons
        let text = self.build_aggregate_text(&activity_counts, total_cost);

        // Build tooltip with per-session breakdown
        let tooltip = self.build_aggregate_tooltip(sessions, total_cost);

        AggregateState {
            text,
            tooltip,
            class: if any_active { "active".to_string() } else { "idle".to_string() },
            alt: if any_active { "active".to_string() } else { "idle".to_string() },
            sessions: sessions.len(),
            total_cost,
        }
    }

    fn build_aggregate_text(&self, counts: &HashMap<String, usize>, total_cost: f64) -> String {
        let mut parts: Vec<String> = Vec::new();

        // Map activities to icons and counts
        let icon_map = [
            ("Thinking", "󰔟"),
            ("Read", "󰈔"),
            ("Edit", "󰏫"),
            ("Write", "󰏫"),
            ("Bash", "󰆍"),
            ("Grep", "󰍉"),
            ("Glob", "󰍉"),
            ("Task", "󰔟"),
        ];

        for (activity, icon) in icon_map {
            if let Some(&count) = counts.get(activity) {
                if count > 0 {
                    parts.push(format!("{} {}", count, icon));
                }
            }
        }

        // Handle Idle separately
        if let Some(&idle_count) = counts.get("Idle") {
            if idle_count > 0 && parts.is_empty() {
                return format!("󰒲 Idle | ${:.2}", total_cost);
            }
        }

        if parts.is_empty() {
            format!("󰒲 Idle | ${:.2}", total_cost)
        } else {
            format!("{} | ${:.2}", parts.join(" "), total_cost)
        }
    }

    fn build_aggregate_tooltip(&self, sessions: &[WaybarState], total_cost: f64) -> String {
        let mut lines = vec![
            format!("{} active sessions | ${:.2} total", sessions.len(), total_cost),
            String::new(),
        ];

        for session in sessions {
            let cwd_short = session.cwd
                .replace(dirs::home_dir().unwrap_or_default().to_str().unwrap_or(""), "~");
            lines.push(format!(
                "{}: {} - {} (${:.2})",
                cwd_short,
                session.model,
                session.activity,
                session.cost
            ));
        }

        lines.join("\n")
    }

    /// Write aggregate state to output file
    pub fn write_aggregate(&self, state: &AggregateState) -> std::io::Result<()> {
        let waybar_state = WaybarState {
            text: state.text.clone(),
            tooltip: state.tooltip.clone(),
            class: state.class.clone(),
            alt: state.alt.clone(),
            cost: state.total_cost,
            ..Default::default()
        };

        waybar_state.write_atomic(&self.output_path)
    }

    /// Clean up stale session files
    pub fn cleanup_stale(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        if let Ok(entries) = fs::read_dir(&self.sessions_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "json").unwrap_or(false) {
                    if let Ok(state) = WaybarState::read_from(&path) {
                        if state.last_activity_time > 0
                            && (now - state.last_activity_time) > self.stale_timeout_secs as i64
                        {
                            let _ = fs::remove_file(&path);
                        }
                    }
                }
            }
        }
    }

    /// Watch sessions directory and update aggregate on changes
    pub fn watch(&self) -> Result<(), Box<dyn std::error::Error>> {
        let (tx, rx) = channel();

        let mut watcher = notify::recommended_watcher(move |res: Result<Event, _>| {
            if let Ok(event) = res {
                if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)) {
                    let _ = tx.send(());
                }
            }
        })?;

        // Ensure directory exists
        fs::create_dir_all(&self.sessions_dir)?;

        watcher.watch(&self.sessions_dir, RecursiveMode::NonRecursive)?;

        eprintln!("Aggregator watching {} for session changes...", self.sessions_dir.display());

        // Initial aggregate
        let state = self.aggregate();
        self.write_aggregate(&state)?;
        let _ = signal_waybar(self.signal);

        loop {
            match rx.recv_timeout(Duration::from_secs(60)) {
                Ok(()) => {
                    // Debounce rapid changes
                    std::thread::sleep(Duration::from_millis(50));

                    // Drain any queued events
                    while rx.try_recv().is_ok() {}

                    self.cleanup_stale();
                    let state = self.aggregate();
                    let _ = self.write_aggregate(&state);
                    let _ = signal_waybar(self.signal);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    // Periodic cleanup
                    self.cleanup_stale();
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        Ok(())
    }
}
```

**Step 2: Add mod declaration in main.rs**

At the top of `main.rs`, add:
```rust
mod aggregator;
```

**Step 3: Verify it compiles**

Run: `nix develop -c cargo build -p waybar-llm-bridge`
Expected: Success

**Step 4: Commit**

```bash
git add crates/waybar-llm-bridge/src/aggregator.rs crates/waybar-llm-bridge/src/main.rs
git commit -m "feat(aggregator): add session aggregation module"
```

---

### Task 2.2: Add --aggregate flag to daemon command

**Files:**
- Modify: `crates/waybar-llm-bridge/src/main.rs`

**Step 1: Update Daemon command**

```rust
    /// Watch transcript and auto-update (daemon mode)
    Daemon {
        /// Watch transcript file for changes
        #[arg(long)]
        log_path: Option<PathBuf>,

        /// Aggregate mode: watch sessions directory instead
        #[arg(long)]
        aggregate: bool,

        /// Sessions directory (for aggregate mode)
        #[arg(long)]
        sessions_dir: Option<PathBuf>,
    },
```

**Step 2: Update main() daemon handling**

```rust
    Commands::Daemon { log_path, aggregate, sessions_dir } => {
        if aggregate {
            let sessions = sessions_dir.unwrap_or(config.sessions_dir);
            handle_daemon_aggregate(&sessions, &state_path, cli.signal)
        } else if let Some(log) = log_path {
            handle_daemon(&log, &state_path, cli.signal)
        } else {
            Err("Either --log-path or --aggregate is required".into())
        }
    }
```

**Step 3: Add handle_daemon_aggregate function**

```rust
fn handle_daemon_aggregate(
    sessions_dir: &PathBuf,
    state_path: &PathBuf,
    signal: u8,
) -> Result<(), Box<dyn std::error::Error>> {
    use aggregator::SessionAggregator;

    let aggregator = SessionAggregator::new(
        sessions_dir.clone(),
        state_path.clone(),
        signal,
    );

    aggregator.watch()
}
```

**Step 4: Verify it compiles**

Run: `nix develop -c cargo build -p waybar-llm-bridge`
Expected: Success

**Step 5: Test manually**

```bash
# Terminal 1: Start aggregator
./target/debug/waybar-llm-bridge daemon --aggregate

# Terminal 2: Simulate events
./target/debug/waybar-llm-bridge event --type submit --session-id test1
./target/debug/waybar-llm-bridge event --type tool-start --tool Read --session-id test1
cat /run/user/$(id -u)/llm_state.json
```

**Step 6: Commit**

```bash
git add crates/waybar-llm-bridge/src/main.rs
git commit -m "feat(daemon): add --aggregate mode for multi-session support"
```

---

## Batch 3: Demo Infrastructure

**Checkpoint:** Demo scripts can be run with `nix run .#demo`

### Task 3.1: Create demo library script

**Files:**
- Create: `demo/lib.sh`

**Step 1: Create demo directory and lib.sh**

```bash
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
```

**Step 2: Make executable**

Run: `chmod +x demo/lib.sh`

**Step 3: Commit**

```bash
git add demo/lib.sh
git commit -m "feat(demo): add demo library with helpers"
```

---

### Task 3.2: Create single-session demo scenario

**Files:**
- Create: `demo/scenarios/single-session.sh`

**Step 1: Create the scenario file**

```bash
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
```

**Step 2: Make executable and test**

```bash
chmod +x demo/scenarios/single-session.sh
nix build && ./demo/scenarios/single-session.sh
```

**Step 3: Commit**

```bash
git add demo/scenarios/single-session.sh
git commit -m "feat(demo): add single-session demo scenario"
```

---

### Task 3.3: Create multi-session demo scenario

**Files:**
- Create: `demo/scenarios/multi-session.sh`

**Step 1: Create the scenario file**

```bash
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
run_and_sync "$BIN event --type submit --session-id $SID_A"
show_state
pace

step 2 $TOTAL "Starting session B in ~/project-b..."
run_and_sync "$BIN event --type submit --session-id $SID_B"
show_state
echo "      (Should show: 2 󰔟 for 2 thinking sessions)"
pace

step 3 $TOTAL "Session A reads file..."
run_and_sync "$BIN event --type tool-start --tool Read --session-id $SID_A"
show_state
echo "      (Should show: 1 󰔟 1 󰈔 for 1 thinking, 1 reading)"
pace

step 4 $TOTAL "Session B edits file..."
run_and_sync "$BIN event --type tool-start --tool Edit --session-id $SID_B"
show_state
echo "      (Should show: 1 󰈔 1 󰏫 for 1 reading, 1 editing)"
pace

step 5 $TOTAL "Session A completes..."
run_and_sync "$BIN event --type stop --session-id $SID_A"
show_state
pace

step 6 $TOTAL "Session B completes..."
run_and_sync "$BIN event --type stop --session-id $SID_B"
show_state
assert_state "activity" "Idle"
pace

step 7 $TOTAL "Aggregation verified"
echo -e "\n${GREEN}✓ Multi-session demo complete!${NC}\n"
```

**Step 2: Make executable**

```bash
chmod +x demo/scenarios/multi-session.sh
```

**Step 3: Commit**

```bash
git add demo/scenarios/multi-session.sh
git commit -m "feat(demo): add multi-session demo scenario"
```

---

### Task 3.4: Create main demo runner

**Files:**
- Create: `demo/demo.sh`

**Step 1: Create the main runner**

```bash
#!/usr/bin/env bash
# Main demo runner

set -e
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib.sh"

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --pace)
            export DEMO_PACE="$2"
            shift 2
            ;;
        --interactive)
            export DEMO_INTERACTIVE=1
            shift
            ;;
        --scenario)
            SCENARIO="$2"
            shift 2
            ;;
        --help)
            echo "Usage: demo.sh [OPTIONS]"
            echo
            echo "Options:"
            echo "  --pace SECONDS     Wait between steps (default: 0)"
            echo "  --interactive      Wait for Enter between steps"
            echo "  --scenario NAME    Run specific scenario (single-session, multi-session)"
            echo
            echo "Scenarios:"
            echo "  single-session     Basic tool activity progression"
            echo "  multi-session      Aggregate view from multiple sessions"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

check_binary

echo -e "${CYAN}"
echo "╔═══════════════════════════════════════╗"
echo "║     llm-waybar Demo                   ║"
echo "║     Claude Code → Waybar Bridge       ║"
echo "╚═══════════════════════════════════════╝"
echo -e "${NC}"

if [[ -n "$SCENARIO" ]]; then
    case $SCENARIO in
        single-session)
            "$SCRIPT_DIR/scenarios/single-session.sh"
            ;;
        multi-session)
            "$SCRIPT_DIR/scenarios/multi-session.sh"
            ;;
        *)
            echo "Unknown scenario: $SCENARIO"
            exit 1
            ;;
    esac
else
    # Run all scenarios
    "$SCRIPT_DIR/scenarios/single-session.sh"
    echo
    "$SCRIPT_DIR/scenarios/multi-session.sh"
fi

echo -e "\n${GREEN}═══ All demos complete! ═══${NC}\n"
```

**Step 2: Make executable**

```bash
chmod +x demo/demo.sh
```

**Step 3: Commit**

```bash
git add demo/demo.sh
git commit -m "feat(demo): add main demo runner with CLI options"
```

---

### Task 3.5: Add demo app to flake.nix

**Files:**
- Modify: `flake.nix`

**Step 1: Add demo app**

In the `apps` section, add:

```nix
      apps = forAllSystems (system: {
        waybar-llm-bridge = {
          type = "app";
          program = "${self.packages.${system}.waybar-llm-bridge}/bin/waybar-llm-bridge";
        };
        default = self.apps.${system}.waybar-llm-bridge;

        # Demo runner
        demo = {
          type = "app";
          program = toString (pkgs.writeShellScript "llm-waybar-demo" ''
            export PATH="${self.packages.${system}.waybar-llm-bridge}/bin:$PATH"
            export DEMO_BIN="${self.packages.${system}.waybar-llm-bridge}/bin/waybar-llm-bridge"
            exec ${./demo/demo.sh} "$@"
          '');
        };
      });
```

Note: You'll need to add `pkgs` binding. Update the apps section:

```nix
      apps = forAllSystems (system:
        let pkgs = mkPkgs system;
        in {
          waybar-llm-bridge = {
            type = "app";
            program = "${self.packages.${system}.waybar-llm-bridge}/bin/waybar-llm-bridge";
          };
          default = self.apps.${system}.waybar-llm-bridge;

          demo = {
            type = "app";
            program = toString (pkgs.writeShellScript "llm-waybar-demo" ''
              export PATH="${self.packages.${system}.waybar-llm-bridge}/bin:$PATH"
              export DEMO_BIN="${self.packages.${system}.waybar-llm-bridge}/bin/waybar-llm-bridge"
              exec ${./demo/demo.sh} "$@"
            '');
          };
        });
```

**Step 2: Test it**

```bash
nix run .#demo -- --help
nix run .#demo -- --pace 1 --scenario single-session
```

**Step 3: Commit**

```bash
git add flake.nix
git commit -m "feat(nix): add demo app to flake"
```

---

## Batch 4: Nix Checks Integration

**Checkpoint:** `nix flake check` runs tests

### Task 4.1: Add cargo test check to flake

**Files:**
- Modify: `flake.nix`

**Step 1: Add checks section**

Add after the `apps` section:

```nix
      checks = forAllSystems (system:
        let pkgs = mkPkgs system;
        in {
          # Rust unit tests
          cargo-test = pkgs.rustPlatform.buildRustPackage {
            pname = "waybar-llm-bridge-test";
            version = "0.1.0";
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;

            # Run tests during build
            checkPhase = ''
              cargo test --release
            '';

            # Don't install anything
            installPhase = "touch $out";
          };

          # Integration tests
          integration-test = pkgs.runCommand "integration-test" {
            buildInputs = [ self.packages.${system}.waybar-llm-bridge pkgs.jq ];
          } ''
            export HOME=$(mktemp -d)
            export XDG_RUNTIME_DIR=$(mktemp -d)
            export LLM_BRIDGE_STATE_PATH="$XDG_RUNTIME_DIR/llm_state.json"
            export LLM_BRIDGE_SESSIONS_DIR="$XDG_RUNTIME_DIR/llm_sessions"

            # Run test script
            ${./test-hooks.sh}

            touch $out
          '';
        });
```

**Step 2: Test it**

```bash
nix flake check
```

**Step 3: Commit**

```bash
git add flake.nix
git commit -m "feat(nix): add cargo test and integration checks"
```

---

### Task 4.2: Update test-hooks.sh for nix check compatibility

**Files:**
- Modify: `test-hooks.sh`

**Step 1: Make test-hooks.sh work without result symlink**

Update the top of `test-hooks.sh`:

```bash
#!/usr/bin/env bash
set -e

# Binary can be provided by DEMO_BIN env var or found in result/
BIN="${DEMO_BIN:-./result/bin/waybar-llm-bridge}"
if [[ ! -x "$BIN" ]]; then
    # Try to find waybar-llm-bridge in PATH
    BIN=$(command -v waybar-llm-bridge 2>/dev/null || echo "")
fi

if [[ -z "$BIN" || ! -x "$BIN" ]]; then
    echo "ERROR: waybar-llm-bridge not found. Run 'nix build' or set DEMO_BIN."
    exit 1
fi

STATE_FILE="${LLM_BRIDGE_STATE_PATH:-/run/user/$(id -u)/llm_state.json}"
```

**Step 2: Verify tests still pass**

```bash
nix build && ./test-hooks.sh
```

**Step 3: Commit**

```bash
git add test-hooks.sh
git commit -m "fix(tests): make test-hooks.sh work with nix check"
```

---

## Batch 5: Documentation & Polish

**Checkpoint:** README updated, ready for release

### Task 5.1: Update README with multi-session docs

**Files:**
- Modify: `README.md`

**Step 1: Add multi-session section to README**

Add after the "Configuration" section:

```markdown
## Multi-Session Support

Run multiple Claude Code sessions and see aggregated status:

### Setup

1. Start the aggregator daemon:
```bash
waybar-llm-bridge daemon --aggregate &
```

2. Update your Claude hooks to include session ID (automatic with `install-hooks`).

### Aggregate Display

When multiple sessions are active:
- **Text:** `2 󰔟 1 󰈔` (2 thinking, 1 reading)
- **Tooltip:** Per-session breakdown with project paths
- **Cost:** Sum of all session costs

### Session Files

Each session writes to:
```
/run/user/$UID/llm_sessions/{session_id}.json
```

Sessions are automatically cleaned up after 5 minutes of inactivity.

## Demo

Run the visual demo:

```bash
# Normal speed
nix run .#demo

# Slow for recording (2 sec between steps)
nix run .#demo -- --pace 2

# Interactive (press Enter for each step)
nix run .#demo -- --interactive

# Specific scenario
nix run .#demo -- --scenario single-session
nix run .#demo -- --scenario multi-session
```
```

**Step 2: Commit**

```bash
git add README.md
git commit -m "docs: add multi-session and demo documentation"
```

---

### Task 5.2: Final integration test

**Step 1: Run full test suite**

```bash
nix flake check
```

**Step 2: Run demo**

```bash
nix run .#demo -- --pace 1
```

**Step 3: Test with real Claude (manual)**

Open two terminals with different projects and verify aggregate view works.

**Step 4: Final commit**

```bash
git add -A
git commit -m "chore: final polish for multi-session and demo release"
```

---

## Summary

| Batch | Tasks | Checkpoint |
|-------|-------|------------|
| 1 | 1.1-1.5 | Sessions write to individual files |
| 2 | 2.1-2.2 | Daemon aggregates sessions |
| 3 | 3.1-3.5 | Demo scripts work with `nix run .#demo` |
| 4 | 4.1-4.2 | `nix flake check` runs tests |
| 5 | 5.1-5.2 | Documentation complete |

**Total: 14 tasks across 5 batches**
