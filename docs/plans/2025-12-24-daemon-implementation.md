# Daemon Architecture Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Eliminate hook latency by implementing a persistent daemon with Unix socket IPC, signal debouncing, and direct token parsing from Claude's `context_window.current_usage`.

**Architecture:** Hooks send fire-and-forget UDP messages to a daemon that maintains in-memory state, debounces waybar signals (16ms window), and caches waybar PID. Fallback to direct mode when daemon unavailable.

**Tech Stack:** Rust, Unix datagram sockets, tokio async runtime, nix crate for signals

---

## Phase 1: Parse context_window.current_usage (Quick Win)

### Task 1.1: Add ContextWindow Structs

**Files:**
- Modify: `crates/waybar-llm-bridge/src/main.rs:89-109`

**Step 1: Add the new structs after CostInfo**

Add these structs to parse the new Claude statusline format:

```rust
#[derive(Debug, Deserialize)]
struct ContextWindow {
    current_usage: Option<CurrentUsage>,
}

#[derive(Debug, Deserialize)]
struct CurrentUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
}
```

**Step 2: Update StatuslineInput struct**

Add `context_window` field to `StatuslineInput`:

```rust
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct StatuslineInput {
    session_id: Option<String>,
    transcript_path: Option<String>,
    cwd: Option<String>,
    model: Option<ModelInfo>,
    cost: Option<CostInfo>,
    context_window: Option<ContextWindow>,  // NEW
}
```

**Step 3: Build to verify syntax**

Run: `cargo build --release -p waybar-llm-bridge`
Expected: SUCCESS

**Step 4: Commit**

```bash
git add crates/waybar-llm-bridge/src/main.rs
git commit -m "feat: add context_window structs for new Claude statusline format"
```

---

### Task 1.2: Use context_window Tokens Instead of Transcript

**Files:**
- Modify: `crates/waybar-llm-bridge/src/main.rs:362-380`

**Step 1: Replace transcript parsing with context_window extraction**

Replace the transcript parsing block in `handle_statusline` with:

```rust
    // Extract token usage from context_window (new Claude statusline format)
    // This is much faster than parsing transcript.jsonl
    if let Some(ref cw) = status_input.context_window {
        if let Some(ref usage) = cw.current_usage {
            state.input_tokens = usage.input_tokens.unwrap_or(0);
            state.output_tokens = usage.output_tokens.unwrap_or(0);
            state.cache_read = usage.cache_read_input_tokens.unwrap_or(0);
            state.cache_write = usage.cache_creation_input_tokens.unwrap_or(0);
        }
    }

    // Fallback: parse transcript only if context_window not available
    if state.input_tokens == 0 && state.output_tokens == 0 {
        if let Some(transcript_path) = status_input.transcript_path.as_ref() {
            let transcript_pathbuf = PathBuf::from(transcript_path);
            if transcript_pathbuf.exists() {
                let provider = ClaudeProvider::new();
                if let Ok(usage) = provider.parse_usage(&transcript_pathbuf) {
                    state.input_tokens = usage.input_tokens;
                    state.output_tokens = usage.output_tokens;
                    state.cache_read = usage.cache_read;
                    state.cache_write = usage.cache_write;
                    if state.cost == 0.0 {
                        state.cost = usage.estimated_cost;
                    }
                }
            }
        }
    }
```

**Step 2: Build to verify**

Run: `cargo build --release -p waybar-llm-bridge`
Expected: SUCCESS

**Step 3: Test with new format**

Run:
```bash
echo '{
  "session_id": "test",
  "model": {"display_name": "Opus 4.5"},
  "cost": {"total_cost_usd": 0.42},
  "context_window": {
    "current_usage": {
      "input_tokens": 8500,
      "output_tokens": 1200,
      "cache_creation_input_tokens": 5000,
      "cache_read_input_tokens": 2000
    }
  }
}' | LLM_BRIDGE_STATE_PATH=/tmp/test_state.json ./target/release/waybar-llm-bridge statusline
```

Expected: `Opus 4.5 | $0.42`

**Step 4: Verify state file has tokens**

Run: `cat /tmp/test_state.json | jq '{input_tokens, output_tokens, cache_read, cache_write}'`

Expected:
```json
{
  "input_tokens": 8500,
  "output_tokens": 1200,
  "cache_read": 2000,
  "cache_write": 5000
}
```

**Step 5: Commit**

```bash
git add crates/waybar-llm-bridge/src/main.rs
git commit -m "feat: parse tokens from context_window.current_usage, fallback to transcript"
```

---

## Phase 2: Socket Infrastructure

### Task 2.1: Add Socket Path Configuration

**Files:**
- Modify: `crates/llm-bridge-core/src/config.rs`

**Step 1: Read current config.rs**

Read the file to understand current structure.

**Step 2: Add socket_path field to Config**

Add after `sessions_dir`:

```rust
    pub socket_path: PathBuf,
```

And in `from_env()`:

```rust
        let socket_path = env::var("LLM_BRIDGE_SOCKET_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| runtime_dir.join("llm-bridge.sock"));
```

**Step 3: Build to verify**

Run: `cargo build --release`
Expected: SUCCESS

**Step 4: Commit**

```bash
git add crates/llm-bridge-core/src/config.rs
git commit -m "feat: add socket_path to Config"
```

---

### Task 2.2: Create Socket Module

**Files:**
- Create: `crates/llm-bridge-core/src/socket.rs`
- Modify: `crates/llm-bridge-core/src/lib.rs`

**Step 1: Create socket.rs with message types**

```rust
use std::io;
use std::os::unix::net::UnixDatagram;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum DaemonMessage {
    Event { event_type: String, tool: Option<String> },
    Status { payload: String },
}

impl DaemonMessage {
    pub fn encode(&self) -> String {
        match self {
            DaemonMessage::Event { event_type, tool } => {
                match tool {
                    Some(t) => format!("EVENT:{}:{}", event_type, t),
                    None => format!("EVENT:{}", event_type),
                }
            }
            DaemonMessage::Status { payload } => format!("STATUS:{}", payload),
        }
    }

    pub fn decode(s: &str) -> Option<Self> {
        if let Some(rest) = s.strip_prefix("EVENT:") {
            let parts: Vec<&str> = rest.splitn(2, ':').collect();
            Some(DaemonMessage::Event {
                event_type: parts[0].to_string(),
                tool: parts.get(1).map(|s| s.to_string()),
            })
        } else if let Some(rest) = s.strip_prefix("STATUS:") {
            Some(DaemonMessage::Status { payload: rest.to_string() })
        } else {
            None
        }
    }
}

/// Try to send message to daemon. Returns Ok(true) if sent, Ok(false) if daemon not available.
pub fn send_to_daemon(socket_path: &Path, message: &DaemonMessage) -> io::Result<bool> {
    let socket = match UnixDatagram::unbound() {
        Ok(s) => s,
        Err(_) => return Ok(false),
    };

    // Non-blocking connect attempt
    socket.set_write_timeout(Some(Duration::from_millis(1)))?;

    let encoded = message.encode();
    match socket.send_to(encoded.as_bytes(), socket_path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::ConnectionRefused => Ok(false),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}
```

**Step 2: Export from lib.rs**

Add to `crates/llm-bridge-core/src/lib.rs`:

```rust
pub mod socket;
pub use socket::{DaemonMessage, send_to_daemon};
```

**Step 3: Build to verify**

Run: `cargo build --release`
Expected: SUCCESS

**Step 4: Commit**

```bash
git add crates/llm-bridge-core/src/socket.rs crates/llm-bridge-core/src/lib.rs
git commit -m "feat: add socket module with DaemonMessage and send_to_daemon"
```

---

### Task 2.3: Update Event Handler to Use Socket

**Files:**
- Modify: `crates/waybar-llm-bridge/src/main.rs`

**Step 1: Add socket import**

Add to imports at top:

```rust
use llm_bridge_core::{socket::{DaemonMessage, send_to_daemon}};
```

**Step 2: Update handle_event to try daemon first**

At the start of `handle_event`, before any file I/O:

```rust
fn handle_event(
    event_type: EventType,
    tool: Option<String>,
    _payload: Option<String>,
    session_id: Option<String>,
    state_path: &PathBuf,
    sessions_dir: &PathBuf,
    signal: u8,
    format: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env();

    // Try daemon first (fire-and-forget, <1ms)
    let event_str = match event_type {
        EventType::Submit => "submit",
        EventType::ToolStart => "tool-start",
        EventType::ToolEnd => "tool-end",
        EventType::Stop => "stop",
    };

    let message = DaemonMessage::Event {
        event_type: event_str.to_string(),
        tool: tool.clone(),
    };

    if send_to_daemon(&config.socket_path, &message).unwrap_or(false) {
        // Daemon handled it, we're done
        return Ok(());
    }

    // Fallback: direct mode (daemon not running)
    // ... rest of existing code unchanged ...
```

**Step 3: Build to verify**

Run: `cargo build --release`
Expected: SUCCESS

**Step 4: Commit**

```bash
git add crates/waybar-llm-bridge/src/main.rs
git commit -m "feat: try daemon socket before fallback to direct mode in handle_event"
```

---

### Task 2.4: Update Statusline Handler to Use Socket

**Files:**
- Modify: `crates/waybar-llm-bridge/src/main.rs`

**Step 1: Update handle_statusline to send to daemon after printing**

After `println!("{}", status_line);` and before state updates, add:

```rust
    // Output a single status line (for Claude Code's statusLine display)
    let status_line = format!("{} | ${:.2}", model_name, cost);
    println!("{}", status_line);

    // Try to send full payload to daemon for async processing
    let message = DaemonMessage::Status { payload: input.clone() };
    if send_to_daemon(&config.socket_path, &message).unwrap_or(false) {
        // Daemon will handle state update and waybar signal
        return Ok(());
    }

    // Fallback: direct mode (daemon not running)
    // ... rest of existing code unchanged ...
```

Note: Need to capture `config` at start of function and keep `input` before parsing.

**Step 2: Refactor handle_statusline for daemon support**

Full refactored function:

```rust
fn handle_statusline(
    state_path: &PathBuf,
    sessions_dir: &PathBuf,
    signal: u8,
    format: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env();
    let stdin = io::stdin();

    if stdin.is_terminal() {
        eprintln!("Error: statusline expects JSON input piped from Claude Code's statusLine hook.");
        eprintln!("This command is not meant to be run directly.");
        eprintln!();
        eprintln!("To install the statusLine hook, run:");
        eprintln!("  waybar-llm-bridge install-hooks");
        return Err("No input provided".into());
    }

    // Read JSON from stdin
    let mut input = String::new();
    for line in stdin.lock().lines() {
        input.push_str(&line?);
    }

    // Parse just enough to output status line quickly
    let status_input: StatuslineInput = serde_json::from_str(&input).unwrap_or(StatuslineInput {
        session_id: None,
        transcript_path: None,
        cwd: None,
        model: None,
        cost: None,
        context_window: None,
    });

    let model_name = status_input
        .model
        .as_ref()
        .and_then(|m| m.display_name.as_ref().or(m.id.as_ref()))
        .map(|s| s.as_str())
        .unwrap_or("Claude");

    let cost = status_input
        .cost
        .as_ref()
        .and_then(|c| c.total_cost_usd)
        .unwrap_or(0.0);

    // Output status line immediately (Claude is waiting for this)
    println!("{} | ${:.2}", model_name, cost);

    // Try to send to daemon for async state update
    let message = DaemonMessage::Status { payload: input.clone() };
    if send_to_daemon(&config.socket_path, &message).unwrap_or(false) {
        return Ok(());
    }

    // Fallback: direct mode
    let mut state = WaybarState::read_from(state_path).unwrap_or_default();
    state.model = model_name.to_string();
    state.cost = cost;

    if let Some(ref sid) = status_input.session_id {
        state.session_id = sid.clone();
    }
    if let Some(ref cwd) = status_input.cwd {
        state.cwd = cwd.clone();
    }

    // Extract tokens from context_window
    if let Some(ref cw) = status_input.context_window {
        if let Some(ref usage) = cw.current_usage {
            state.input_tokens = usage.input_tokens.unwrap_or(0);
            state.output_tokens = usage.output_tokens.unwrap_or(0);
            state.cache_read = usage.cache_read_input_tokens.unwrap_or(0);
            state.cache_write = usage.cache_creation_input_tokens.unwrap_or(0);
        }
    }

    // Fallback transcript parsing
    if state.input_tokens == 0 && state.output_tokens == 0 {
        if let Some(transcript_path) = status_input.transcript_path.as_ref() {
            let transcript_pathbuf = PathBuf::from(transcript_path);
            if transcript_pathbuf.exists() {
                let provider = ClaudeProvider::new();
                if let Ok(usage) = provider.parse_usage(&transcript_pathbuf) {
                    state.input_tokens = usage.input_tokens;
                    state.output_tokens = usage.output_tokens;
                    state.cache_read = usage.cache_read;
                    state.cache_write = usage.cache_write;
                    if state.cost == 0.0 {
                        state.cost = usage.estimated_cost;
                    }
                }
            }
        }
    }

    state.text = state.compute_text(format);
    state.tooltip = state.compute_tooltip();
    let _ = state.write_session_file(sessions_dir);
    state.write_atomic(state_path)?;
    let _ = signal_waybar(signal);

    Ok(())
}
```

**Step 3: Build to verify**

Run: `cargo build --release`
Expected: SUCCESS

**Step 4: Test fallback mode still works**

Run test with no daemon:
```bash
echo '{"model":{"display_name":"Test"},"cost":{"total_cost_usd":0.01}}' | \
  LLM_BRIDGE_STATE_PATH=/tmp/test.json ./target/release/waybar-llm-bridge statusline
```
Expected: `Test | $0.01`

**Step 5: Commit**

```bash
git add crates/waybar-llm-bridge/src/main.rs
git commit -m "feat: statusline sends to daemon, falls back to direct mode"
```

---

## Phase 3: Daemon Implementation

### Task 3.1: Create Daemon State Structure

**Files:**
- Create: `crates/waybar-llm-bridge/src/daemon.rs`
- Modify: `crates/waybar-llm-bridge/src/main.rs`

**Step 1: Create daemon.rs with state management**

```rust
use std::collections::HashMap;
use std::os::unix::net::UnixDatagram;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use std::fs;

use llm_bridge_core::{WaybarState, AgentPhase, signal::signal_waybar, socket::DaemonMessage};

const DEBOUNCE_MS: u64 = 16;
const MAX_DEBOUNCE_MS: u64 = 50;
const DISK_FLUSH_MS: u64 = 100;

pub struct Daemon {
    socket_path: PathBuf,
    state_path: PathBuf,
    sessions_dir: PathBuf,
    signal_num: u8,
    format: String,

    // In-memory state
    state: WaybarState,

    // Waybar PID cache
    waybar_pid: Option<i32>,
    pid_cache_time: Instant,

    // Debouncing
    pending_signal: bool,
    first_event_time: Option<Instant>,
    last_event_time: Instant,

    // Disk write batching
    dirty: bool,
    last_disk_write: Instant,
}

impl Daemon {
    pub fn new(
        socket_path: PathBuf,
        state_path: PathBuf,
        sessions_dir: PathBuf,
        signal_num: u8,
        format: String,
    ) -> Self {
        // Load existing state if available
        let state = WaybarState::read_from(&state_path).unwrap_or_default();

        Self {
            socket_path,
            state_path,
            sessions_dir,
            signal_num,
            format,
            state,
            waybar_pid: None,
            pid_cache_time: Instant::now(),
            pending_signal: false,
            first_event_time: None,
            last_event_time: Instant::now(),
            dirty: false,
            last_disk_write: Instant::now(),
        }
    }

    pub fn handle_message(&mut self, msg: DaemonMessage) {
        match msg {
            DaemonMessage::Event { event_type, tool } => {
                self.handle_event(&event_type, tool);
            }
            DaemonMessage::Status { payload } => {
                self.handle_status(&payload);
            }
        }

        self.last_event_time = Instant::now();
        if self.first_event_time.is_none() {
            self.first_event_time = Some(Instant::now());
        }
        self.pending_signal = true;
        self.dirty = true;
    }

    fn handle_event(&mut self, event_type: &str, tool: Option<String>) {
        let phase = match event_type {
            "submit" => AgentPhase::Thinking,
            "tool-start" => AgentPhase::ToolUse {
                tool: tool.unwrap_or_else(|| "unknown".to_string()),
            },
            "tool-end" => AgentPhase::Thinking,
            "stop" => AgentPhase::Idle,
            _ => return,
        };

        let (activity, class, alt) = match &phase {
            AgentPhase::Idle => ("Idle".to_string(), "idle".to_string(), "idle".to_string()),
            AgentPhase::Thinking => ("Thinking".to_string(), "thinking".to_string(), "active".to_string()),
            AgentPhase::ToolUse { tool } => {
                let truncated = if tool.len() > 20 {
                    format!("{}...", &tool[..17])
                } else {
                    tool.clone()
                };
                (truncated, "tool-active".to_string(), "active".to_string())
            }
            AgentPhase::Error { message } => {
                (format!("Error: {}", message), "error".to_string(), "error".to_string())
            }
        };

        self.state.activity = activity;
        self.state.class = class;
        self.state.alt = alt;
        self.state.last_activity_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        self.state.text = self.state.compute_text(&self.format);
    }

    fn handle_status(&mut self, payload: &str) {
        // Parse the status payload
        #[derive(serde::Deserialize)]
        struct StatusPayload {
            session_id: Option<String>,
            cwd: Option<String>,
            model: Option<ModelInfo>,
            cost: Option<CostInfo>,
            context_window: Option<ContextWindow>,
        }

        #[derive(serde::Deserialize)]
        struct ModelInfo {
            id: Option<String>,
            display_name: Option<String>,
        }

        #[derive(serde::Deserialize)]
        struct CostInfo {
            total_cost_usd: Option<f64>,
        }

        #[derive(serde::Deserialize)]
        struct ContextWindow {
            current_usage: Option<CurrentUsage>,
        }

        #[derive(serde::Deserialize)]
        struct CurrentUsage {
            input_tokens: Option<u64>,
            output_tokens: Option<u64>,
            cache_creation_input_tokens: Option<u64>,
            cache_read_input_tokens: Option<u64>,
        }

        if let Ok(status) = serde_json::from_str::<StatusPayload>(payload) {
            if let Some(model) = status.model {
                self.state.model = model.display_name
                    .or(model.id)
                    .unwrap_or_else(|| "Claude".to_string());
            }

            if let Some(cost) = status.cost {
                self.state.cost = cost.total_cost_usd.unwrap_or(0.0);
            }

            if let Some(sid) = status.session_id {
                self.state.session_id = sid;
            }

            if let Some(cwd) = status.cwd {
                self.state.cwd = cwd;
            }

            if let Some(cw) = status.context_window {
                if let Some(usage) = cw.current_usage {
                    self.state.input_tokens = usage.input_tokens.unwrap_or(0);
                    self.state.output_tokens = usage.output_tokens.unwrap_or(0);
                    self.state.cache_read = usage.cache_read_input_tokens.unwrap_or(0);
                    self.state.cache_write = usage.cache_creation_input_tokens.unwrap_or(0);
                }
            }

            self.state.text = self.state.compute_text(&self.format);
            self.state.tooltip = self.state.compute_tooltip();
        }
    }

    /// Check if we should signal waybar (debounce logic)
    pub fn should_signal(&self) -> bool {
        if !self.pending_signal {
            return false;
        }

        let now = Instant::now();
        let since_last = now.duration_since(self.last_event_time);
        let since_first = self.first_event_time
            .map(|t| now.duration_since(t))
            .unwrap_or(Duration::ZERO);

        // Signal if: debounce window passed OR max delay reached
        since_last >= Duration::from_millis(DEBOUNCE_MS)
            || since_first >= Duration::from_millis(MAX_DEBOUNCE_MS)
    }

    /// Signal waybar and reset debounce state
    pub fn do_signal(&mut self) {
        if let Some(pid) = self.waybar_pid {
            // Try cached PID first
            if self.signal_pid(pid).is_err() {
                // PID stale, refresh
                self.refresh_waybar_pid();
                if let Some(new_pid) = self.waybar_pid {
                    let _ = self.signal_pid(new_pid);
                }
            }
        } else {
            self.refresh_waybar_pid();
            if let Some(pid) = self.waybar_pid {
                let _ = self.signal_pid(pid);
            }
        }

        self.pending_signal = false;
        self.first_event_time = None;
    }

    fn signal_pid(&self, pid: i32) -> Result<(), nix::errno::Errno> {
        use nix::sys::signal::{self, Signal};
        use nix::unistd::Pid;
        use nix::libc;

        let sig = Signal::try_from(libc::SIGRTMIN() + self.signal_num as i32)
            .map_err(|_| nix::errno::Errno::EINVAL)?;
        signal::kill(Pid::from_raw(pid), sig)
    }

    fn refresh_waybar_pid(&mut self) {
        use std::process::Command;

        if let Ok(output) = Command::new("pgrep").arg("-x").arg("waybar").output() {
            if output.status.success() {
                if let Ok(s) = String::from_utf8(output.stdout) {
                    if let Some(pid) = s.lines().next().and_then(|l| l.trim().parse().ok()) {
                        self.waybar_pid = Some(pid);
                        self.pid_cache_time = Instant::now();
                        return;
                    }
                }
            }
        }
        self.waybar_pid = None;
    }

    /// Check if we should flush to disk
    pub fn should_flush(&self) -> bool {
        self.dirty && self.last_disk_write.elapsed() >= Duration::from_millis(DISK_FLUSH_MS)
    }

    /// Flush state to disk
    pub fn do_flush(&mut self) {
        let _ = self.state.write_session_file(&self.sessions_dir);
        let _ = self.state.write_atomic(&self.state_path);
        self.dirty = false;
        self.last_disk_write = Instant::now();
    }

    /// Bind and return the socket
    pub fn bind_socket(&self) -> std::io::Result<UnixDatagram> {
        // Remove old socket if exists
        let _ = fs::remove_file(&self.socket_path);

        let socket = UnixDatagram::bind(&self.socket_path)?;
        socket.set_nonblocking(true)?;

        // Set permissions to user-only
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            let _ = fs::set_permissions(&self.socket_path, perms);
        }

        Ok(socket)
    }
}
```

**Step 2: Add mod declaration to main.rs**

Add at top of main.rs:

```rust
mod daemon;
```

**Step 3: Build to verify**

Run: `cargo build --release`
Expected: SUCCESS (may have warnings about unused, that's ok)

**Step 4: Commit**

```bash
git add crates/waybar-llm-bridge/src/daemon.rs crates/waybar-llm-bridge/src/main.rs
git commit -m "feat: add Daemon struct with state management and debouncing"
```

---

### Task 3.2: Implement Daemon Run Loop

**Files:**
- Modify: `crates/waybar-llm-bridge/src/daemon.rs`

**Step 1: Add run method to Daemon**

Add this method to the `impl Daemon` block:

```rust
    /// Main daemon loop
    pub fn run(&mut self) -> std::io::Result<()> {
        let socket = self.bind_socket()?;

        eprintln!("llm-bridge daemon listening on {:?}", self.socket_path);

        let mut buf = [0u8; 65536];

        loop {
            // Try to receive a message (non-blocking)
            match socket.recv(&mut buf) {
                Ok(n) => {
                    if let Ok(s) = std::str::from_utf8(&buf[..n]) {
                        if let Some(msg) = DaemonMessage::decode(s) {
                            self.handle_message(msg);
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No message available, that's fine
                }
                Err(e) => {
                    eprintln!("Socket error: {}", e);
                }
            }

            // Check debounce timer and signal if ready
            if self.should_signal() {
                self.do_signal();
            }

            // Check disk flush timer
            if self.should_flush() {
                self.do_flush();
            }

            // Small sleep to prevent busy-waiting
            std::thread::sleep(Duration::from_millis(1));
        }
    }
```

**Step 2: Build to verify**

Run: `cargo build --release`
Expected: SUCCESS

**Step 3: Commit**

```bash
git add crates/waybar-llm-bridge/src/daemon.rs
git commit -m "feat: add daemon run loop with non-blocking socket polling"
```

---

### Task 3.3: Wire Up Daemon Command

**Files:**
- Modify: `crates/waybar-llm-bridge/src/main.rs`

**Step 1: Update Commands enum**

The existing `Daemon` variant needs updating. Find and modify:

```rust
    /// Run as background daemon (new high-performance mode)
    Daemon {
        /// Watch transcript file for changes (legacy mode)
        #[arg(long)]
        log_path: Option<PathBuf>,

        /// Aggregate mode: watch sessions directory (legacy mode)
        #[arg(long)]
        aggregate: bool,

        /// Sessions directory (for aggregate mode)
        #[arg(long)]
        sessions_dir: Option<PathBuf>,

        /// Run new socket-based daemon (default if no other flags)
        #[arg(long)]
        socket: bool,
    },
```

**Step 2: Update daemon handling in main**

Update the daemon match arm:

```rust
        Commands::Daemon { log_path, aggregate, sessions_dir, socket } => {
            // New socket daemon mode (default when no legacy flags)
            if socket || (log_path.is_none() && !aggregate) {
                handle_daemon_socket(&state_path, &config.sessions_dir, cli.signal, &format)
            } else if aggregate {
                let sessions = sessions_dir.unwrap_or(config.sessions_dir);
                handle_daemon_aggregate(&sessions, &state_path, cli.signal)
            } else if let Some(log) = log_path {
                handle_daemon(&log, &state_path, cli.signal)
            } else {
                Err("Either --log-path, --aggregate, or --socket is required".into())
            }
        }
```

**Step 3: Add handle_daemon_socket function**

Add after `handle_daemon_aggregate`:

```rust
fn handle_daemon_socket(
    state_path: &PathBuf,
    sessions_dir: &PathBuf,
    signal: u8,
    format: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use daemon::Daemon;

    let config = Config::from_env();

    let mut daemon = Daemon::new(
        config.socket_path,
        state_path.clone(),
        sessions_dir.clone(),
        signal,
        format.to_string(),
    );

    daemon.run()?;
    Ok(())
}
```

**Step 4: Build to verify**

Run: `cargo build --release`
Expected: SUCCESS

**Step 5: Quick manual test**

In terminal 1:
```bash
LLM_BRIDGE_STATE_PATH=/tmp/daemon_test.json ./target/release/waybar-llm-bridge daemon
```

In terminal 2:
```bash
# This should fail silently (no daemon protocol yet in client)
./target/release/waybar-llm-bridge event --type submit
```

**Step 6: Commit**

```bash
git add crates/waybar-llm-bridge/src/main.rs
git commit -m "feat: wire up socket daemon command"
```

---

## Phase 4: Home Manager Module

### Task 4.1: Add Home Manager Module to Flake

**Files:**
- Modify: `flake.nix`

**Step 1: Add homeManagerModules output**

After `checks = forAllSystems (...)`, add:

```nix
      homeManagerModules.default = { config, lib, pkgs, ... }:
        let
          cfg = config.services.llm-bridge;
        in {
          options.services.llm-bridge = {
            enable = lib.mkEnableOption "LLM Waybar Bridge daemon";

            package = lib.mkOption {
              type = lib.types.package;
              default = self.packages.${pkgs.stdenv.hostPlatform.system}.waybar-llm-bridge;
              description = "The waybar-llm-bridge package to use";
            };
          };

          config = lib.mkIf cfg.enable {
            systemd.user.services.llm-bridge = {
              Unit = {
                Description = "LLM Waybar Bridge Daemon";
                After = [ "graphical-session.target" ];
                PartOf = [ "graphical-session.target" ];
              };
              Service = {
                ExecStart = "${cfg.package}/bin/waybar-llm-bridge daemon";
                Restart = "on-failure";
                RestartSec = 1;
              };
              Install.WantedBy = [ "graphical-session.target" ];
            };

            home.packages = [ cfg.package ];
          };
        };
```

**Step 2: Verify flake syntax**

Run: `nix flake check --no-build`
Expected: SUCCESS (or warnings about missing checks)

**Step 3: Commit**

```bash
git add flake.nix
git commit -m "feat: add Home Manager module with systemd service"
```

---

### Task 4.2: Update Nix Wrapper with Socket Path

**Files:**
- Modify: `flake.nix`

**Step 1: Update postInstall wrapper**

Find the `postInstall` in the package definition and add socket path:

```nix
            postInstall = ''
              wrapProgram $out/bin/waybar-llm-bridge \
                --run 'export LLM_BRIDGE_STATE_PATH=''${LLM_BRIDGE_STATE_PATH:-"/run/user/$(id -u)/llm_state.json"}' \
                --run 'export LLM_BRIDGE_SOCKET_PATH=''${LLM_BRIDGE_SOCKET_PATH:-"/run/user/$(id -u)/llm-bridge.sock"}' \
                --run 'export LLM_BRIDGE_SIGNAL=''${LLM_BRIDGE_SIGNAL:-"8"}' \
                --run 'export LLM_BRIDGE_TRANSCRIPT_DIR=''${LLM_BRIDGE_TRANSCRIPT_DIR:-"$HOME/.claude/projects"}'
            '';
```

**Step 2: Build to verify**

Run: `nix build`
Expected: SUCCESS

**Step 3: Commit**

```bash
git add flake.nix
git commit -m "feat: add socket path to nix wrapper"
```

---

## Phase 5: Testing

### Task 5.1: Add Integration Test for Daemon

**Files:**
- Modify: `test-hooks.sh`

**Step 1: Add daemon test section**

Add after existing tests:

```bash
echo ""
echo "=== Test 8: Daemon mode ==="

# Start daemon in background
DAEMON_STATE="/tmp/daemon_test_$$.json"
DAEMON_SOCKET="/tmp/daemon_test_$$.sock"
rm -f "$DAEMON_STATE" "$DAEMON_SOCKET"

LLM_BRIDGE_STATE_PATH="$DAEMON_STATE" \
LLM_BRIDGE_SOCKET_PATH="$DAEMON_SOCKET" \
$BIN daemon &
DAEMON_PID=$!
sleep 0.5  # Give daemon time to start

# Send events via socket
LLM_BRIDGE_SOCKET_PATH="$DAEMON_SOCKET" \
LLM_BRIDGE_STATE_PATH="$DAEMON_STATE" \
$BIN event --type submit

sleep 0.1  # Let daemon process

# Check state was updated
ACTIVITY=$(cat "$DAEMON_STATE" | jq -r '.activity')
if [ "$ACTIVITY" = "Thinking" ]; then
    echo "PASS: Daemon processed event correctly"
else
    echo "FAIL: Expected activity 'Thinking', got '$ACTIVITY'"
    kill $DAEMON_PID 2>/dev/null
    exit 1
fi

# Cleanup
kill $DAEMON_PID 2>/dev/null
rm -f "$DAEMON_STATE" "$DAEMON_SOCKET"
```

**Step 2: Run tests**

Run: `./test-hooks.sh`
Expected: All tests pass including new daemon test

**Step 3: Commit**

```bash
git add test-hooks.sh
git commit -m "test: add daemon integration test"
```

---

### Task 5.2: Run Full Test Suite

**Step 1: Run cargo tests**

Run: `cargo test --release`
Expected: All tests pass

**Step 2: Run nix checks**

Run: `nix flake check`
Expected: All checks pass

**Step 3: Final commit**

```bash
git add -A
git commit -m "chore: complete daemon implementation" --allow-empty
```

---

## Summary

After completing all tasks, you will have:

1. **context_window parsing** - Tokens extracted directly from Claude's new statusline format
2. **Socket-based IPC** - Hooks return in <1ms by sending UDP to daemon
3. **Signal debouncing** - Rapid events batched into single waybar refresh
4. **PID caching** - No pgrep on hot path
5. **Graceful fallback** - Direct mode when daemon unavailable
6. **Home Manager module** - Easy NixOS integration with systemd service
