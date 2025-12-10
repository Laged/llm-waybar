# Rust Project Structure Design

## Overview

Provider-based workspace architecture for `waybar-llm-bridge` - a Rust binary that bridges LLM agents (Claude Code now, others later) to Waybar status bar.

## Workspace Structure

```
llm-waybar/
├── Cargo.toml              # Workspace manifest
├── crates/
│   ├── llm-bridge-core/    # Traits, state machine, Waybar signaling
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── config.rs     # Env-based configuration
│   │       ├── state.rs      # WaybarState struct + serialization
│   │       ├── signal.rs     # pkill SIGRTMIN+N logic
│   │       └── provider.rs   # LlmProvider trait
│   │
│   ├── llm-bridge-claude/  # Claude Code-specific implementation
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── hooks.rs      # Hook event parsing
│   │       ├── transcript.rs # transcript.jsonl parsing
│   │       └── usage.rs      # Token/cost calculation
│   │
│   └── waybar-llm-bridge/  # CLI binary
│       └── src/
│           └── main.rs       # clap CLI
│
├── flake.nix
└── docs/
```

## Core Abstractions

### LlmProvider Trait (`llm-bridge-core`)

```rust
pub trait LlmProvider {
    fn name(&self) -> &'static str;
    fn parse_event(&self, event_type: &str, payload: &str) -> Result<LlmEvent>;
    fn parse_usage(&self, log_path: &Path) -> Result<UsageMetrics>;
}
```

### WaybarState (`llm-bridge-core`)

```rust
#[derive(Serialize, Deserialize)]
pub struct WaybarState {
    pub text: String,       // "Thinking...", "Bash"
    pub tooltip: String,    // "Tokens: 15k | Cost: $0.23"
    pub class: String,      // "thinking", "tool-active", "idle"
    pub alt: String,        // For Waybar's {alt} format
    pub percentage: u8,     // Context window usage
}

pub enum AgentPhase {
    Idle,
    Thinking,
    ToolUse { tool: String },
    Error { message: String },
}
```

### Config (`llm-bridge-core`)

```rust
pub struct Config {
    pub state_path: PathBuf,      // LLM_BRIDGE_STATE_PATH
    pub signal: u8,               // LLM_BRIDGE_SIGNAL
    pub transcript_dir: PathBuf,  // LLM_BRIDGE_TRANSCRIPT_DIR
}
```

## CLI Commands

```
waybar-llm-bridge [OPTIONS] <COMMAND>

Options:
    --state-path <PATH>    Override state file path
    --signal <N>           Override signal number

Commands:
    event        Handle hook events (--type, --tool, --payload)
    sync-usage   Parse transcript for token/cost metrics
    status       Output current state as Waybar JSON
    daemon       Watch transcript and auto-update
```

## Dependencies

| Crate | Dependencies |
|-------|--------------|
| `llm-bridge-core` | serde, serde_json, nix, thiserror |
| `llm-bridge-claude` | llm-bridge-core, rev_lines, dirs |
| `waybar-llm-bridge` | llm-bridge-core, llm-bridge-claude, clap, notify, tokio |

## Nix Configuration

All parameters configurable via `flake.nix` using `wrapProgram`:

```nix
postInstall = ''
  wrapProgram $out/bin/waybar-llm-bridge \
    --set-default LLM_BRIDGE_STATE_PATH "/run/user/$(id -u)/llm_state.json" \
    --set-default LLM_BRIDGE_SIGNAL "8" \
    --set-default LLM_BRIDGE_TRANSCRIPT_DIR "$HOME/.claude/projects"
'';
```

## Integration

### Claude Code Hooks (`~/.claude/settings.json`)

- `UserPromptSubmit` → `event --type submit`
- `PreToolUse` → `event --type tool-start --tool "$CLAUDE_TOOL_NAME"`
- `PostToolUse` → `event --type tool-end --tool "$CLAUDE_TOOL_NAME"`
- `Stop` → `event --type stop && sync-usage`

### Waybar Module

```json
"custom/llm": {
    "format": "{}",
    "return-type": "json",
    "exec": "cat $LLM_BRIDGE_STATE_PATH",
    "interval": "once",
    "signal": 8
}
```

## Future Extensions

- Additional providers: `llm-bridge-copilot`, `llm-bridge-cursor`
- Home Manager module for config generation
- MCP proxy mode for real-time interception
