# Architecture: High-Performance LLM-Waybar Bridge (Rust)

## 1. Executive Summary

This document defines the architecture for `waybar-llm-bridge`, a Rust-based integration layer connecting autonomous LLM agents (specifically Claude Code) to the Waybar status bar. 

Building upon the "Push-Signal-Pull" pattern defined in the [Research Overview](../research/intro.md), this system leverages Rust's performance and memory safety to handle high-throughput log parsing (for token/cost tracking) and low-latency state updates (for UI responsiveness). It replaces the reference Python implementation with a single, statically linked binary that serves as both the Hook Handler and the State Manager.

## 2. System Architecture

The system consists of three primary components:

1.  **The Agent (Claude Code)**: Operates in the terminal. It emits:
    *   **Lifecycle Events**: via `settings.json` Hooks (e.g., `PreToolUse`, `Stop`).
    *   **Usage Data**: via `transcript.jsonl` logs (Token counts, Duration).
2.  **The Bridge (Rust Binary)**:
    *   **Ingestor**: Receives synchronous hook events and parses asynchronous logs.
    *   **State Machine**: Aggregates current status (e.g., "Thinking", "Tool: grep") and cumulative metrics (Cost, Tokens).
    *   **Broadcaster**: Atomically updates a JSON state file and signals Waybar.
3.  **The Viewer (Waybar)**:
    *   **Display**: Renders the JSON state.
    *   **Interactivity**: Allows user to click-to-focus the agent or inspect details.

### 2.1 Data Flow Diagram

```mermaid
graph TD
    subgraph "Agent Layer"
        C[Claude Code] -- Hook (Exec) --> B_Hook[Bridge: Hook Handler]
        C -- Writes --> Logs[transcript.jsonl]
    end

    subgraph "Bridge Layer (Rust)"
        B_Hook -- Update State --> State[In-Memory/File State]
        Logs -- Watch/Parse --> B_Watch[Bridge: Usage Analyzer]
        B_Watch -- Update Metrics --> State
        State -- Atomic Write --> JSON[/tmp/llm_state.json]
        B_Hook -- SIGRTMIN+8 --> W[Waybar Process]
    end

    subgraph "Presentation Layer"
        W -- Read --> JSON
        W -- Render --> Display[Status Bar]
    end
```

## 3. Component Design

### 3.1 The Rust Bridge (`waybar-llm-bridge`)

The core is a single Rust binary with two modes of operation: `update` (triggered by hooks) and `watch` (optional daemon for heavy log analysis).

#### 3.1.1 Command-Line Interface (CLI)

```bash
# Handlers for Claude Code Hooks
waybar-llm-bridge event --type start --prompt "..."
waybar-llm-bridge event --type tool-use --tool "bash" --input "ls -la"
waybar-llm-bridge event --type stop

# Daemon mode (Optional, for heavy log parsing)
waybar-llm-bridge daemon --log-path ~/.claude/projects/current/transcript.jsonl
```

#### 3.1.2 Internal State Struct

To ensure "blazingly fast" serialization, we use `serde` with a flat structure optimized for Waybar.

```rust
#[derive(Serialize, Deserialize)]
struct WaybarState {
    text: String,       // "Thinking...", "Idle"
    tooltip: String,    // "Tokens: 15k | Cost: $0.23\nLast Tool: grep"
    class: String,      // "thinking", "tool-active", "error"
    alt: String,        // "idle", "busy"
    percentage: u8,     // Optional: Context window usage %
}
```

#### 3.1.3 Token & Cost Tracking (The `ccusage` Integration)

Unlike simple status updates, token tracking requires parsing the `transcript.jsonl`.
*   **Strategy**: On `PostToolUse` or `Stop` events, the bridge reads the *last N lines* of the transcript (using `seek` from end of file for performance) to find the latest usage statistics.
*   **Performance**: Rust's file I/O allows parsing GB-sized logs in milliseconds, which is critical for calculating cumulative session costs without lag.
*   **Metrics**:
    *   Input Tokens (Cache Read vs. Cache Write)
    *   Output Tokens
    *   Cost calculation based on hardcoded model rates (e.g., Sonnet 3.5 prices).

### 3.2 Waybar Configuration

The integration relies on `return-type: "json"` and `signal` for instant updates.

```jsonc
"custom/llm": {
    "format": "{}", 
    "return-type": "json",
    "exec": "cat /tmp/llm_state.json", // Zero-latency read
    "interval": "once",                // Disable polling
    "signal": 8,                       // RT Signal 8
    "on-click": "swaymsg '[app_id=\"claude-terminal\"] focus'"
}
```

## 4. Implementation Details (Rust Crate Ecosystem)

To achieve the design goals, we will leverage the following crates:

*   **`serde` / `serde_json`**: For robust JSON handling of Claude logs and Waybar output.
*   **`clap`**: For parsing Hook arguments.
*   **`nix`**: For sending Unix Signals (`kill -SIGRTMIN+8`) safely.
*   **`notify` (Optional)**: If we implement a watcher daemon instead of relying solely on Hooks.
*   **`rev_lines`**: To efficiently read `transcript.jsonl` from the end to get the latest token usage without reading the whole file.
*   **`dirs`**: To resolve `~/.claude` paths cross-platform.

## 5. Deployment & Reproducibility

Following the `llm-agents.nix` philosophy:

1.  **Flake-based Build**: The Rust binary is built via a Nix Flake, ensuring `cargo` and `rustc` versions are pinned.
2.  **Wrapper Script**: A Nix wrapper generates the `settings.json` for Claude Code, automatically injecting the absolute path to the compiled `waybar-llm-bridge` binary into the `hooks` configuration. This prevents "binary not found" errors.

## 6. Comparison with Existing Solutions

| Feature | Python Script (Intro.md) | `ccusage` | `waybar-ai-usage` | **Rust Bridge (Proposed)** |
| :--- | :--- | :--- | :--- | :--- |
| **Language** | Python | TypeScript | TypeScript/Shell | **Rust** |
| **Latency** | Medium (Interpreter startup) | Medium | High (Polling) | **Low (Native Binary)** |
| **State Source** | Hooks | Logs | Browser Cookies | **Hooks + Logs** |
| **Token Tracking** | No | **Yes** | Yes (Approx) | **Yes (Exact)** |
| **Waybar Signal** | Yes | No | No | **Yes** |

## 7. Future Work: MCP Inspector

As mentioned in the research, future iterations could implement an **MCP Proxy** in Rust. Instead of relying on Claude Code's file logs, the Rust bridge would act as a middleman server between Claude and its tools, intercepting JSON-RPC messages to count tokens and visualize activity in real-time with microsecond precision.
