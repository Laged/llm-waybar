# Fix Statusline Display: Preserve Activity State & Add Configurable Format

**Created:** 2025-12-10
**Status:** Planning
**Priority:** High

## Problem Statement

The `statusline` command overwrites the activity state set by event hooks, causing the waybar display to always show the model name instead of activity status (Thinking, Tool use, etc.).

### Current Behavior
1. `UserPromptSubmit` hook fires → sets `text: "Thinking..."`
2. `statusline` hook fires → overwrites with `text: "Opus 4.5"` (model name)
3. Waybar always shows model name, never activity status

### Desired Behavior
- Activity status from events should be preserved
- Statusline should update cost/tokens without overwriting activity
- Display format should be configurable: `Model | Status | Tokens | Cost`

## Design Decisions

### State Structure Enhancement

Current `WaybarState`:
```rust
pub struct WaybarState {
    pub text: String,      // Gets overwritten - problem!
    pub tooltip: String,
    pub class: String,
    pub alt: String,
    pub percentage: u8,
}
```

Enhanced `WaybarState`:
```rust
pub struct WaybarState {
    // Separate concerns - each field updated independently
    pub model: String,           // Set by statusline
    pub activity: String,        // Set by events (Idle, Thinking, Read, Edit, etc.)
    pub cost: f64,               // Set by statusline
    pub input_tokens: u64,       // Set by statusline (if available)
    pub output_tokens: u64,      // Set by statusline (if available)
    pub cache_read: u64,         // Set by statusline (if available)
    pub cache_write: u64,        // Set by statusline (if available)

    // Computed from above based on format string
    pub text: String,            // Computed on read
    pub tooltip: String,
    pub class: String,
    pub alt: String,
    pub percentage: u8,
}
```

### Display Format Configuration

Environment variable or config file for format string:
```
LLM_BRIDGE_FORMAT="{model} | {activity} | {tokens}K | ${cost:.2}"
```

Format placeholders:
- `{model}` - Model display name (e.g., "Opus 4.5")
- `{activity}` - Current activity (Idle, Thinking, Read, Edit, Bash, etc.)
- `{cost}` - Session cost in USD
- `{cost:.2}` - Cost with 2 decimal places
- `{tokens}` - Total tokens (input + output) in K
- `{input}` - Input tokens
- `{output}` - Output tokens
- `{cache_read}` - Cache read tokens
- `{cache_write}` - Cache write tokens
- `{limit}` - Remaining daily/hourly limit (future: requires API)

### Claude Code StatusLine JSON

Claude Code pipes this JSON to statusline:
```json
{
  "session_id": "abc123",
  "transcript_path": "/home/user/.claude/projects/hash/transcript.jsonl",
  "cwd": "/home/user/project",
  "model": {
    "id": "claude-opus-4-5-20251101",
    "display_name": "Opus 4.5"
  },
  "cost": {
    "total_cost_usd": 2.5161
  }
}
```

Note: Token counts are NOT in this JSON. To get tokens, we need to parse the transcript file.

## Implementation Plan

### Batch 1: Core State Refactoring
**Checkpoint:** State structure updated, events and statusline work independently

- [ ] **1.1** Update `WaybarState` struct in `llm-bridge-core/src/state.rs`
  - Add `model`, `activity`, `cost`, token fields
  - Keep `text` as computed field
  - Add `format` field for display format string
  - Implement `compute_text()` method using format string

- [ ] **1.2** Update `handle_event()` in `waybar-llm-bridge/src/main.rs`
  - Read existing state
  - Only update `activity` and `class` fields
  - Preserve `model`, `cost`, token fields
  - Call `compute_text()` before writing

- [ ] **1.3** Update `handle_statusline()` in `waybar-llm-bridge/src/main.rs`
  - Read existing state
  - Only update `model`, `cost` fields
  - Preserve `activity` field
  - Call `compute_text()` before writing

### Batch 2: Token Parsing from Transcript
**Checkpoint:** Token counts extracted and displayed

- [ ] **2.1** Enhance `StatuslineInput` struct
  - Keep existing fields
  - Use `transcript_path` to parse tokens on-demand

- [ ] **2.2** Add token extraction in `handle_statusline()`
  - If `transcript_path` is provided, parse last N entries
  - Sum up token usage from transcript
  - Update state with token counts

- [ ] **2.3** Update tooltip to show detailed token breakdown
  - Format: "Input: 12K | Output: 3K | Cache R: 45K | Cache W: 2K"

### Batch 3: Configurable Format String
**Checkpoint:** Users can customize waybar display format

- [ ] **3.1** Add format configuration
  - Environment variable: `LLM_BRIDGE_FORMAT`
  - Default: `"{activity} | ${cost:.2}"`
  - Store in `Config` struct

- [ ] **3.2** Implement format string parser
  - Simple placeholder replacement
  - Support format specifiers for numbers (`.2` for decimals, `K` for thousands)

- [ ] **3.3** Add CLI flag `--format` to override
  - `waybar-llm-bridge status --format "{model}|{activity}"`

### Batch 4: Activity State Improvements
**Checkpoint:** Rich activity states with tool names

- [ ] **4.1** Improve tool activity display
  - Instead of generic "ToolUse", show tool name: "Read", "Edit", "Bash", etc.
  - Truncate long tool names

- [ ] **4.2** Add activity timeout
  - If no event for 60s, revert to "Idle"
  - Store `last_activity_time` in state

- [ ] **4.3** Add activity icons (optional)
  - Map activities to Nerd Font icons
  - Thinking: 󰔟, Read: 󰈔, Edit: 󰏫, Bash: 󰆍, Idle: 󰒲

### Batch 5: Testing & Documentation
**Checkpoint:** Ready for release

- [ ] **5.1** Add unit tests for format string parsing
- [ ] **5.2** Add integration test for event + statusline flow
- [ ] **5.3** Update README with new configuration options
- [ ] **5.4** Test with nixos-config integration

## File Changes Summary

| File | Changes |
|------|---------|
| `llm-bridge-core/src/state.rs` | New state fields, compute_text(), format parsing |
| `llm-bridge-core/src/config.rs` | Add format string config |
| `waybar-llm-bridge/src/main.rs` | Fix handle_event, handle_statusline to merge state |
| `llm-bridge-claude/src/transcript.rs` | Ensure token parsing works for statusline |

## Example Output After Fix

**Waybar display:** `Thinking | $2.52 󰚩`

**Waybar display (custom format):** `Opus 4.5 | Edit | 15K | $2.52 󰚩`

**Tooltip:**
```
Model: Opus 4.5
Status: Thinking
Session Cost: $2.5161
Tokens: 12,450 in / 3,201 out
Cache: 45,000 read / 2,100 write
```

## Migration Notes

- State file format changes - old state files will be read with defaults
- Environment variable `LLM_BRIDGE_FORMAT` is new, defaults preserve current behavior
- Hooks in `~/.claude/settings.json` don't need changes
