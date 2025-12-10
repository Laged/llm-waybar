# Command Reference: waybar-llm-bridge

This document details the CLI commands exposed by `waybar-llm-bridge`, their expected inputs, outputs, and how they integrate into the Claude Code and Waybar ecosystems.

## 1. CLI Overview

The binary provides a suite of commands to manage the state of the AI agent visualization. It is designed to be stateless (persisting data to `/tmp`) and high-performance.

**Usage Syntax:**
```bash
waybar-llm-bridge [GLOBAL_OPTIONS] <COMMAND> [ARGS]
```

**Global Options:**
- `--state-path <PATH>`: Override default state file location (Default: `/tmp/llm_state.json`).
- `--verbose`: Enable debug logging to stderr.

---

## 2. Detailed Commands

### 2.1 `event`
The primary write-operation. It is invoked by Claude Code's lifecycle hooks to trigger a state transition.

**Syntax:**
```bash
waybar-llm-bridge event --type <EVENT_TYPE> [PAYLOAD_OPTIONS]
```

**Supported Event Types:**

#### A. `submit`
Triggered when the user presses Enter on a prompt.
- **Flags:**
  - `--prompt <STRING>`: The text of the user's query.
- **Action:** Sets state to "Thinking", sets tooltip to the prompt text.
- **Example:**
  ```bash
  waybar-llm-bridge event --type submit --prompt "Refactor the authentication module"
  ```

#### B. `tool-start`
Triggered via `PreToolUse` hook.
- **Flags:**
  - `--tool <STRING>`: Name of the tool (e.g., "Bash", "ReadFile").
  - `--input <JSON_STRING>`: The arguments passed to the tool.
- **Action:** Sets state to "Active", updates icon to tool-specific symbol (e.g., terminal icon for Bash).
- **Example:**
  ```bash
  waybar-llm-bridge event --type tool-start --tool "Bash" --input '{"command": "ls -la"}'
  ```

#### C. `tool-end`
Triggered via `PostToolUse` hook.
- **Flags:**
  - `--tool <STRING>`: Name of the tool.
  - `--error <STRING>`: (Optional) Error message if tool failed.
- **Action:** 
  1. Triggers a **Log Scan** (see `sync-usage`) to update token counts. 
  2. Updates state to "Thinking" (if chain continues) or "Idle".
- **Example:**
  ```bash
  waybar-llm-bridge event --type tool-end --tool "Bash"
  ```

#### D. `stop`
Triggered when the agent finishes its turn and awaits user input.
- **Action:** Sets state to "Idle", clears active tool indicators.
- **Example:**
  ```bash
  waybar-llm-bridge event --type stop
  ```

---

### 2.2 `sync-usage`
Scans the Claude Code transcript to calculate and update token usage and estimated costs. This command allows the bridge to be "stateless" regarding tokens—it always derives truth from logs.

**Syntax:**
```bash
waybar-llm-bridge sync-usage --log-path <PATH_TO_TRANSCRIPT>
```

- **Behavior:**
  1. Opens the `transcript.jsonl`.
  2. Uses `rev_lines` to scan from the end of the file.
  3. Aggregates `tokens_input` and `tokens_output`.
  4. Updates the `tooltip` field in the state file with Cost/Token stats.
- **Output:** None (Updates state file silently).

---

### 2.3 `status`
Outputs the current state in Waybar-compatible JSON. Useful for debugging or if Waybar is configured to run this command directly instead of `cat`.

**Syntax:**
```bash
waybar-llm-bridge status
```

**Output Example (STDOUT):**
```json
{
  "text": "  Thinking...",
  "tooltip": "Phase: Planning\nTask: Refactor auth\n\nTokens: 1,240 (In) / 305 (Out)\nEst. Cost: $0.0052",
  "class": "thinking",
  "alt": "active",
  "percentage": 15
}
```

---

## 3. Integration & Invocation

### 3.1 Exposed to Claude Code (`settings.json`)
Claude Code calls the `event` command via its Hook system. The arguments are constructed dynamically using jq-like syntax or environment variables provided by the hook context.

**Configuration Snippet:**
```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "*",
        "hooks": [
          {
            "type": "command",
            "command": "waybar-llm-bridge event --type tool-start --tool \"$CLAUDE_TOOL_NAME\" --input \"$CLAUDE_TOOL_INPUT\""
          }
        ]
      }
    ],
    "PostToolUse": [
      {
        "matcher": "*",
        "hooks": [
          {
            "type": "command",
            "command": "waybar-llm-bridge event --type tool-end --tool \"$CLAUDE_TOOL_NAME\" && waybar-llm-bridge sync-usage --log-path \"$CLAUDE_TRANSCRIPT_PATH\""
          }
        ]
      }
    ]
  }
}
```

### 3.2 Exposed to Waybar (`config.jsonc`)
Waybar "pulls" the data. We utilize the `return-type: json` feature.

**Method A: Direct File Read (Fastest)**
Waybar reads the JSON file written by the bridge. The bridge sends `SIGRTMIN+8` to force a refresh.
```json
"custom/llm": {
    "format": "{}",
    "return-type": "json",
    "exec": "cat /tmp/llm_state.json", 
    "interval": "once",
    "signal": 8
}
```

**Method B: Command Execution**
Waybar runs the bridge to get status. Slower (process spawn overhead) but simpler to debug.
```json
"custom/llm": {
    "format": "{}",
    "return-type": "json",
    "exec": "waybar-llm-bridge status",
    "interval": "once",
    "signal": 8
}
```

### 3.3 The Signal Mechanism
Every time `waybar-llm-bridge event` runs, it executes the equivalent of:
```rust
// Rust pseudo-code
nix::sys::signal::kill(
    Pid::from_raw(waybar_pid), 
    nix::sys::signal::Signal::SIGRTMIN + 8
);
```
This ensures that the millisecond the agent changes state, the bar updates.

```