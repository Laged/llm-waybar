# waybar-llm-bridge Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a Rust CLI that bridges Claude Code hooks to Waybar status bar via atomic JSON state and real-time signals.

**Architecture:** Provider-based workspace with `llm-bridge-core` (traits, state, signals), `llm-bridge-claude` (Claude-specific parsing), and `waybar-llm-bridge` (CLI binary). Push-Signal-Pull pattern: hooks push events → atomic JSON write → SIGRTMIN+8 → Waybar reads.

**Tech Stack:** Rust, serde, clap, nix (signals), rev_lines, notify, tokio

---

## Task 1: Workspace Setup

**Files:**
- Modify: `Cargo.toml` (root workspace manifest)
- Create: `crates/llm-bridge-core/Cargo.toml`
- Create: `crates/llm-bridge-core/src/lib.rs`
- Create: `crates/llm-bridge-claude/Cargo.toml`
- Create: `crates/llm-bridge-claude/src/lib.rs`
- Create: `crates/waybar-llm-bridge/Cargo.toml`
- Create: `crates/waybar-llm-bridge/src/main.rs`
- Delete: `src/main.rs` (cargo init default)

**Step 1: Create workspace Cargo.toml**

Replace root `Cargo.toml` with:

```toml
[workspace]
resolver = "2"
members = [
    "crates/llm-bridge-core",
    "crates/llm-bridge-claude",
    "crates/waybar-llm-bridge",
]

[workspace.package]
version = "0.1.0"
edition = "2021"
license = "MIT"
```

**Step 2: Create llm-bridge-core crate**

Create `crates/llm-bridge-core/Cargo.toml`:

```toml
[package]
name = "llm-bridge-core"
version.workspace = true
edition.workspace = true

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
nix = { version = "0.29", features = ["signal", "process"] }
thiserror = "2.0"
```

Create `crates/llm-bridge-core/src/lib.rs`:

```rust
pub mod config;
pub mod state;
pub mod signal;
pub mod provider;

pub use config::Config;
pub use state::{WaybarState, AgentPhase};
pub use provider::{LlmProvider, LlmEvent, UsageMetrics};
```

**Step 3: Create llm-bridge-claude crate**

Create `crates/llm-bridge-claude/Cargo.toml`:

```toml
[package]
name = "llm-bridge-claude"
version.workspace = true
edition.workspace = true

[dependencies]
llm-bridge-core = { path = "../llm-bridge-core" }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
rev_lines = "0.3"
dirs = "5.0"
thiserror = "2.0"
```

Create `crates/llm-bridge-claude/src/lib.rs`:

```rust
pub mod hooks;
pub mod transcript;
pub mod usage;

mod provider;
pub use provider::ClaudeProvider;
```

**Step 4: Create waybar-llm-bridge binary crate**

Create `crates/waybar-llm-bridge/Cargo.toml`:

```toml
[package]
name = "waybar-llm-bridge"
version.workspace = true
edition.workspace = true

[[bin]]
name = "waybar-llm-bridge"
path = "src/main.rs"

[dependencies]
llm-bridge-core = { path = "../llm-bridge-core" }
llm-bridge-claude = { path = "../llm-bridge-claude" }
clap = { version = "4.5", features = ["derive"] }
tokio = { version = "1.0", features = ["full"] }
notify = "7.0"
```

Create `crates/waybar-llm-bridge/src/main.rs`:

```rust
fn main() {
    println!("waybar-llm-bridge");
}
```

**Step 5: Remove old src/ and verify workspace builds**

Run:
```bash
rm -rf src/
nix develop -c cargo build
```

Expected: Build succeeds with 3 crates compiled.

**Step 6: Commit**

```bash
git add -A
git commit -m "feat: set up workspace with 3 crates"
```

---

## Task 2: Core Config Module

**Files:**
- Create: `crates/llm-bridge-core/src/config.rs`
- Test: Manual verification via cargo build

**Step 1: Write config module**

Create `crates/llm-bridge-core/src/config.rs`:

```rust
use std::env;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub state_path: PathBuf,
    pub signal: u8,
    pub transcript_dir: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            state_path: default_state_path(),
            signal: 8,
            transcript_dir: default_transcript_dir(),
        }
    }
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            state_path: env::var("LLM_BRIDGE_STATE_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| default_state_path()),
            signal: env::var("LLM_BRIDGE_SIGNAL")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(8),
            transcript_dir: env::var("LLM_BRIDGE_TRANSCRIPT_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| default_transcript_dir()),
        }
    }
}

fn default_state_path() -> PathBuf {
    if let Ok(runtime_dir) = env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(runtime_dir).join("llm_state.json")
    } else {
        PathBuf::from("/tmp/llm_state.json")
    }
}

fn default_transcript_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude/projects")
}
```

**Step 2: Add dirs dependency to core**

Update `crates/llm-bridge-core/Cargo.toml`, add to `[dependencies]`:

```toml
dirs = "5.0"
```

**Step 3: Verify build**

Run:
```bash
nix develop -c cargo build
```

Expected: Build succeeds.

**Step 4: Commit**

```bash
git add -A
git commit -m "feat(core): add Config with env-based defaults"
```

---

## Task 3: Core State Module

**Files:**
- Create: `crates/llm-bridge-core/src/state.rs`

**Step 1: Write state module**

Create `crates/llm-bridge-core/src/state.rs`:

```rust
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::Path;

use crate::provider::UsageMetrics;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WaybarState {
    pub text: String,
    pub tooltip: String,
    pub class: String,
    pub alt: String,
    pub percentage: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentPhase {
    Idle,
    Thinking,
    ToolUse { tool: String },
    Error { message: String },
}

impl WaybarState {
    pub fn from_phase(phase: &AgentPhase, usage: Option<&UsageMetrics>) -> Self {
        let (text, class, alt) = match phase {
            AgentPhase::Idle => ("Idle".to_string(), "idle".to_string(), "idle".to_string()),
            AgentPhase::Thinking => ("Thinking...".to_string(), "thinking".to_string(), "active".to_string()),
            AgentPhase::ToolUse { tool } => (tool.clone(), "tool-active".to_string(), "active".to_string()),
            AgentPhase::Error { message } => (format!("Error: {}", message), "error".to_string(), "error".to_string()),
        };

        let tooltip = usage
            .map(|u| format!(
                "Tokens: {} in / {} out\nCost: ${:.4}",
                u.input_tokens, u.output_tokens, u.estimated_cost
            ))
            .unwrap_or_default();

        Self {
            text,
            tooltip,
            class,
            alt,
            percentage: 0,
        }
    }

    pub fn write_atomic(&self, path: &Path) -> std::io::Result<()> {
        let tmp_path = path.with_extension("tmp");
        let json = serde_json::to_string(self)?;

        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;

        fs::rename(&tmp_path, path)?;
        Ok(())
    }

    pub fn read_from(path: &Path) -> std::io::Result<Self> {
        let content = fs::read_to_string(path)?;
        serde_json::from_str(&content).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}
```

**Step 2: Verify build**

Run:
```bash
nix develop -c cargo build
```

Expected: Build succeeds.

**Step 3: Commit**

```bash
git add -A
git commit -m "feat(core): add WaybarState with atomic write"
```

---

## Task 4: Core Signal Module

**Files:**
- Create: `crates/llm-bridge-core/src/signal.rs`

**Step 1: Write signal module**

Create `crates/llm-bridge-core/src/signal.rs`:

```rust
use nix::sys::signal::{self, Signal};
use nix::unistd::Pid;
use std::process::Command;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SignalError {
    #[error("Failed to find waybar process")]
    WaybarNotFound,
    #[error("Failed to send signal: {0}")]
    SendFailed(#[from] nix::errno::Errno),
    #[error("Invalid signal number: {0}")]
    InvalidSignal(u8),
}

pub fn signal_waybar(signal_num: u8) -> Result<(), SignalError> {
    let pids = find_waybar_pids()?;

    let sig = Signal::try_from(libc::SIGRTMIN() + signal_num as i32)
        .map_err(|_| SignalError::InvalidSignal(signal_num))?;

    for pid in pids {
        signal::kill(Pid::from_raw(pid), sig)?;
    }

    Ok(())
}

fn find_waybar_pids() -> Result<Vec<i32>, SignalError> {
    let output = Command::new("pgrep")
        .arg("-x")
        .arg("waybar")
        .output()
        .map_err(|_| SignalError::WaybarNotFound)?;

    if !output.status.success() {
        return Err(SignalError::WaybarNotFound);
    }

    let pids: Vec<i32> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|s| s.trim().parse().ok())
        .collect();

    if pids.is_empty() {
        return Err(SignalError::WaybarNotFound);
    }

    Ok(pids)
}
```

**Step 2: Verify build**

Run:
```bash
nix develop -c cargo build
```

Expected: Build succeeds.

**Step 3: Commit**

```bash
git add -A
git commit -m "feat(core): add signal_waybar with SIGRTMIN+N"
```

---

## Task 5: Core Provider Trait

**Files:**
- Create: `crates/llm-bridge-core/src/provider.rs`

**Step 1: Write provider trait**

Create `crates/llm-bridge-core/src/provider.rs`:

```rust
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProviderError {
    #[error("Failed to parse event: {0}")]
    ParseEvent(String),
    #[error("Failed to parse usage: {0}")]
    ParseUsage(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub enum LlmEvent {
    Submit { prompt: Option<String> },
    ToolStart { tool: String, input: Option<String> },
    ToolEnd { tool: String, error: Option<String> },
    Stop,
}

#[derive(Debug, Clone, Default)]
pub struct UsageMetrics {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub estimated_cost: f64,
}

pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn parse_event(&self, event_type: &str, payload: Option<&str>) -> Result<LlmEvent, ProviderError>;
    fn parse_usage(&self, log_path: &Path) -> Result<UsageMetrics, ProviderError>;
}
```

**Step 2: Verify build**

Run:
```bash
nix develop -c cargo build
```

Expected: Build succeeds.

**Step 3: Commit**

```bash
git add -A
git commit -m "feat(core): add LlmProvider trait and types"
```

---

## Task 6: Claude Provider - Hooks Parser

**Files:**
- Create: `crates/llm-bridge-claude/src/hooks.rs`
- Create: `crates/llm-bridge-claude/src/provider.rs`

**Step 1: Write hooks parser**

Create `crates/llm-bridge-claude/src/hooks.rs`:

```rust
use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
pub struct ClaudeHookPayload {
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub tool_input: Option<serde_json::Value>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

impl ClaudeHookPayload {
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        if json.trim().is_empty() {
            return Ok(Self::default());
        }
        serde_json::from_str(json)
    }
}
```

**Step 2: Write Claude provider**

Create `crates/llm-bridge-claude/src/provider.rs`:

```rust
use std::path::Path;
use llm_bridge_core::{LlmProvider, LlmEvent, UsageMetrics, ProviderError};
use crate::hooks::ClaudeHookPayload;
use crate::transcript::parse_transcript_tail;
use crate::usage::calculate_cost;

pub struct ClaudeProvider;

impl ClaudeProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ClaudeProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmProvider for ClaudeProvider {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn parse_event(&self, event_type: &str, payload: Option<&str>) -> Result<LlmEvent, ProviderError> {
        let hook_payload = payload
            .map(ClaudeHookPayload::from_json)
            .transpose()
            .map_err(|e| ProviderError::ParseEvent(e.to_string()))?
            .unwrap_or_default();

        match event_type {
            "submit" => Ok(LlmEvent::Submit {
                prompt: hook_payload.prompt,
            }),
            "tool-start" => Ok(LlmEvent::ToolStart {
                tool: hook_payload.tool_name.unwrap_or_else(|| "unknown".to_string()),
                input: hook_payload.tool_input.map(|v| v.to_string()),
            }),
            "tool-end" => Ok(LlmEvent::ToolEnd {
                tool: hook_payload.tool_name.unwrap_or_else(|| "unknown".to_string()),
                error: hook_payload.error,
            }),
            "stop" => Ok(LlmEvent::Stop),
            other => Err(ProviderError::ParseEvent(format!("Unknown event type: {}", other))),
        }
    }

    fn parse_usage(&self, log_path: &Path) -> Result<UsageMetrics, ProviderError> {
        let entries = parse_transcript_tail(log_path, 100)?;
        Ok(calculate_cost(&entries))
    }
}
```

**Step 3: Verify build**

Run:
```bash
nix develop -c cargo build
```

Expected: Build fails (transcript/usage modules not yet created). Continue to next task.

**Step 4: Commit (partial)**

```bash
git add -A
git commit -m "feat(claude): add hooks parser and provider skeleton"
```

---

## Task 7: Claude Provider - Transcript Parser

**Files:**
- Create: `crates/llm-bridge-claude/src/transcript.rs`

**Step 1: Write transcript parser**

Create `crates/llm-bridge-claude/src/transcript.rs`:

```rust
use serde::Deserialize;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use llm_bridge_core::ProviderError;

#[derive(Debug, Deserialize)]
pub struct TranscriptEntry {
    #[serde(rename = "type")]
    pub entry_type: Option<String>,
    #[serde(default)]
    pub message: Option<TranscriptMessage>,
}

#[derive(Debug, Deserialize)]
pub struct TranscriptMessage {
    #[serde(default)]
    pub usage: Option<TokenUsage>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct TokenUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
}

pub fn parse_transcript_tail(path: &Path, max_lines: usize) -> Result<Vec<TokenUsage>, ProviderError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut usages = Vec::new();

    // Read all lines and take last max_lines
    let lines: Vec<_> = reader.lines().collect();
    let start = lines.len().saturating_sub(max_lines);

    for line_result in lines.into_iter().skip(start) {
        let line = line_result?;
        if line.trim().is_empty() {
            continue;
        }

        if let Ok(entry) = serde_json::from_str::<TranscriptEntry>(&line) {
            if let Some(message) = entry.message {
                if let Some(usage) = message.usage {
                    usages.push(usage);
                }
            }
        }
    }

    Ok(usages)
}
```

**Step 2: Verify build**

Run:
```bash
nix develop -c cargo build
```

Expected: Build fails (usage module not yet created). Continue.

**Step 3: Commit (partial)**

```bash
git add -A
git commit -m "feat(claude): add transcript.jsonl parser"
```

---

## Task 8: Claude Provider - Usage Calculator

**Files:**
- Create: `crates/llm-bridge-claude/src/usage.rs`

**Step 1: Write usage calculator**

Create `crates/llm-bridge-claude/src/usage.rs`:

```rust
use llm_bridge_core::UsageMetrics;
use crate::transcript::TokenUsage;

// Claude Sonnet 3.5 pricing (per million tokens)
const INPUT_PRICE: f64 = 3.0;
const OUTPUT_PRICE: f64 = 15.0;
const CACHE_READ_PRICE: f64 = 0.30;
const CACHE_WRITE_PRICE: f64 = 3.75;

pub fn calculate_cost(usages: &[TokenUsage]) -> UsageMetrics {
    let mut total = UsageMetrics::default();

    for usage in usages {
        total.input_tokens += usage.input_tokens;
        total.output_tokens += usage.output_tokens;
        total.cache_read += usage.cache_read_input_tokens;
        total.cache_write += usage.cache_creation_input_tokens;
    }

    total.estimated_cost =
        (total.input_tokens as f64 * INPUT_PRICE / 1_000_000.0) +
        (total.output_tokens as f64 * OUTPUT_PRICE / 1_000_000.0) +
        (total.cache_read as f64 * CACHE_READ_PRICE / 1_000_000.0) +
        (total.cache_write as f64 * CACHE_WRITE_PRICE / 1_000_000.0);

    total
}
```

**Step 2: Verify full build**

Run:
```bash
nix develop -c cargo build
```

Expected: Build succeeds with all 3 crates.

**Step 3: Commit**

```bash
git add -A
git commit -m "feat(claude): add usage/cost calculator"
```

---

## Task 9: CLI Binary - Command Structure

**Files:**
- Modify: `crates/waybar-llm-bridge/src/main.rs`

**Step 1: Write CLI with clap**

Replace `crates/waybar-llm-bridge/src/main.rs`:

```rust
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use llm_bridge_core::{Config, WaybarState, AgentPhase, signal::signal_waybar};
use llm_bridge_claude::ClaudeProvider;
use llm_bridge_core::LlmProvider;

#[derive(Parser)]
#[command(name = "waybar-llm-bridge")]
#[command(about = "Bridge LLM agents to Waybar status bar")]
struct Cli {
    #[arg(long, env = "LLM_BRIDGE_STATE_PATH")]
    state_path: Option<PathBuf>,

    #[arg(long, env = "LLM_BRIDGE_SIGNAL", default_value = "8")]
    signal: u8,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Handle hook events from LLM agents
    Event {
        #[arg(long, value_enum)]
        r#type: EventType,
        #[arg(long)]
        tool: Option<String>,
        #[arg(long)]
        payload: Option<String>,
    },
    /// Sync usage metrics from transcript logs
    SyncUsage {
        #[arg(long)]
        log_path: PathBuf,
    },
    /// Output current state as Waybar JSON
    Status,
    /// Watch transcript and auto-update (daemon mode)
    Daemon {
        #[arg(long)]
        log_path: PathBuf,
    },
}

#[derive(Clone, ValueEnum)]
enum EventType {
    Submit,
    ToolStart,
    ToolEnd,
    Stop,
}

fn main() {
    let cli = Cli::parse();
    let config = Config::from_env();
    let state_path = cli.state_path.unwrap_or(config.state_path);

    let result = match cli.command {
        Commands::Event { r#type, tool, payload } => {
            handle_event(r#type, tool, payload, &state_path, cli.signal)
        }
        Commands::SyncUsage { log_path } => {
            handle_sync_usage(&log_path, &state_path, cli.signal)
        }
        Commands::Status => {
            handle_status(&state_path)
        }
        Commands::Daemon { log_path } => {
            handle_daemon(&log_path, &state_path, cli.signal)
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

fn handle_event(
    event_type: EventType,
    tool: Option<String>,
    _payload: Option<String>,
    state_path: &PathBuf,
    signal: u8,
) -> Result<(), Box<dyn std::error::Error>> {
    let phase = match event_type {
        EventType::Submit => AgentPhase::Thinking,
        EventType::ToolStart => AgentPhase::ToolUse {
            tool: tool.unwrap_or_else(|| "unknown".to_string()),
        },
        EventType::ToolEnd => AgentPhase::Thinking,
        EventType::Stop => AgentPhase::Idle,
    };

    let state = WaybarState::from_phase(&phase, None);
    state.write_atomic(state_path)?;

    let _ = signal_waybar(signal); // Ignore if waybar not running
    Ok(())
}

fn handle_sync_usage(
    log_path: &PathBuf,
    state_path: &PathBuf,
    signal: u8,
) -> Result<(), Box<dyn std::error::Error>> {
    let provider = ClaudeProvider::new();
    let usage = provider.parse_usage(log_path)?;

    // Read current state and update tooltip
    let mut state = WaybarState::read_from(state_path).unwrap_or_default();
    state.tooltip = format!(
        "Tokens: {} in / {} out\nCache: {} read / {} write\nCost: ${:.4}",
        usage.input_tokens,
        usage.output_tokens,
        usage.cache_read,
        usage.cache_write,
        usage.estimated_cost
    );

    state.write_atomic(state_path)?;
    let _ = signal_waybar(signal);
    Ok(())
}

fn handle_status(state_path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let state = WaybarState::read_from(state_path).unwrap_or_default();
    println!("{}", serde_json::to_string(&state)?);
    Ok(())
}

fn handle_daemon(
    _log_path: &PathBuf,
    _state_path: &PathBuf,
    _signal: u8,
) -> Result<(), Box<dyn std::error::Error>> {
    // TODO: Implement file watcher with notify crate
    eprintln!("Daemon mode not yet implemented");
    Ok(())
}
```

**Step 2: Verify build and test CLI**

Run:
```bash
nix develop -c cargo build
nix develop -c cargo run -- --help
nix develop -c cargo run -- event --type submit
nix develop -c cargo run -- status
```

Expected: CLI works, creates state file, outputs JSON.

**Step 3: Commit**

```bash
git add -A
git commit -m "feat(cli): add event, sync-usage, status commands"
```

---

## Task 10: Daemon Mode with File Watcher

**Files:**
- Modify: `crates/waybar-llm-bridge/src/main.rs`

**Step 1: Implement daemon with notify**

Add to `main.rs` (replace `handle_daemon` function):

```rust
use notify::{Watcher, RecursiveMode, Event, EventKind};
use std::sync::mpsc::channel;
use std::time::Duration;

fn handle_daemon(
    log_path: &PathBuf,
    state_path: &PathBuf,
    signal: u8,
) -> Result<(), Box<dyn std::error::Error>> {
    let (tx, rx) = channel();

    let mut watcher = notify::recommended_watcher(move |res: Result<Event, _>| {
        if let Ok(event) = res {
            if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                let _ = tx.send(());
            }
        }
    })?;

    watcher.watch(log_path, RecursiveMode::NonRecursive)?;

    eprintln!("Watching {} for changes...", log_path.display());

    let provider = ClaudeProvider::new();

    loop {
        match rx.recv_timeout(Duration::from_secs(60)) {
            Ok(()) => {
                if let Ok(usage) = provider.parse_usage(log_path) {
                    let mut state = WaybarState::read_from(state_path).unwrap_or_default();
                    state.tooltip = format!(
                        "Tokens: {} in / {} out\nCost: ${:.4}",
                        usage.input_tokens,
                        usage.output_tokens,
                        usage.estimated_cost
                    );
                    let _ = state.write_atomic(state_path);
                    let _ = signal_waybar(signal);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    Ok(())
}
```

**Step 2: Add missing import**

Add at top of `main.rs`:

```rust
use notify::{Watcher, RecursiveMode, Event, EventKind};
use std::sync::mpsc::channel;
use std::time::Duration;
```

**Step 3: Verify build**

Run:
```bash
nix develop -c cargo build
```

Expected: Build succeeds.

**Step 4: Commit**

```bash
git add -A
git commit -m "feat(cli): add daemon mode with file watcher"
```

---

## Task 11: Update flake.nix for Rust Build

**Files:**
- Modify: `flake.nix`

**Step 1: Update flake with rustPlatform build**

Read current flake and update to include Rust package build:

```nix
{
  description = "llm-waybar development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    fenix = {
      url = "https://flakehub.com/f/nix-community/fenix/0.1";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, fenix, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      mkPkgs = system: import nixpkgs {
        inherit system;
        overlays = [ self.overlays.default ];
      };
    in {
      overlays.default = final: prev: {
        rustToolchain = with fenix.packages.${prev.stdenv.hostPlatform.system};
          combine (with stable; [ rustc cargo clippy rustfmt rust-src ]);
      };

      devShells = forAllSystems (system:
        let pkgs = mkPkgs system;
        in {
          default = pkgs.mkShell {
            packages = with pkgs; [
              rustToolchain
              pkg-config
              openssl
              rust-analyzer
            ];

            env = {
              RUST_SRC_PATH = "${pkgs.rustToolchain}/lib/rustlib/src/rust/library";
            };
          };
        });

      packages = forAllSystems (system:
        let pkgs = mkPkgs system;
        in {
          waybar-llm-bridge = pkgs.rustPlatform.buildRustPackage {
            pname = "waybar-llm-bridge";
            version = "0.1.0";
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;

            nativeBuildInputs = [ pkgs.makeWrapper ];

            postInstall = ''
              wrapProgram $out/bin/waybar-llm-bridge \
                --set-default LLM_BRIDGE_STATE_PATH "/run/user/\$(id -u)/llm_state.json" \
                --set-default LLM_BRIDGE_SIGNAL "8" \
                --set-default LLM_BRIDGE_TRANSCRIPT_DIR "\$HOME/.claude/projects"
            '';
          };

          default = self.packages.${system}.waybar-llm-bridge;
        });

      apps = forAllSystems (system: {
        waybar-llm-bridge = {
          type = "app";
          program = "${self.packages.${system}.waybar-llm-bridge}/bin/waybar-llm-bridge";
        };
        default = self.apps.${system}.waybar-llm-bridge;
      });
    };
}
```

**Step 2: Generate Cargo.lock and test nix build**

Run:
```bash
nix develop -c cargo generate-lockfile
nix build
```

Expected: Builds successfully, produces `result/bin/waybar-llm-bridge`.

**Step 3: Test the built binary**

Run:
```bash
./result/bin/waybar-llm-bridge --help
./result/bin/waybar-llm-bridge status
```

Expected: CLI works with env defaults.

**Step 4: Commit**

```bash
git add -A
git commit -m "feat(nix): add rustPlatform build with wrapProgram defaults"
```

---

## Task 12: Final Integration Test

**Step 1: Manual end-to-end test**

Run these commands to verify full flow:

```bash
# Build
nix build

# Test event flow
./result/bin/waybar-llm-bridge event --type submit
./result/bin/waybar-llm-bridge status
# Should show: {"text":"Thinking...","tooltip":"","class":"thinking","alt":"active","percentage":0}

./result/bin/waybar-llm-bridge event --type tool-start --tool Bash
./result/bin/waybar-llm-bridge status
# Should show: {"text":"Bash","tooltip":"","class":"tool-active","alt":"active","percentage":0}

./result/bin/waybar-llm-bridge event --type stop
./result/bin/waybar-llm-bridge status
# Should show: {"text":"Idle","tooltip":"","class":"idle","alt":"idle","percentage":0}
```

**Step 2: Commit final state**

```bash
git add -A
git commit -m "chore: complete initial implementation"
```

---

## Summary

| Task | Description | Files |
|------|-------------|-------|
| 1 | Workspace setup | Cargo.toml, 3 crate scaffolds |
| 2 | Config module | core/config.rs |
| 3 | State module | core/state.rs |
| 4 | Signal module | core/signal.rs |
| 5 | Provider trait | core/provider.rs |
| 6 | Claude hooks | claude/hooks.rs, provider.rs |
| 7 | Transcript parser | claude/transcript.rs |
| 8 | Usage calculator | claude/usage.rs |
| 9 | CLI commands | waybar-llm-bridge/main.rs |
| 10 | Daemon mode | main.rs (notify) |
| 11 | Nix build | flake.nix |
| 12 | Integration test | Manual verification |
