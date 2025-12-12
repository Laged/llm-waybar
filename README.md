# llm-waybar

A Waybar integration bridge for Claude Code that displays real-time LLM activity, token usage, and cost tracking in your status bar.

## Overview

`llm-waybar` is a bridge between Claude Code (Anthropic's CLI) and Waybar that provides:

- Real-time activity status (Idle, Thinking, Tool usage)
- Model information display
- Token usage tracking (input, output, cache read/write)
- Session cost monitoring
- Configurable display format with icons
- Activity timeout (auto-reset to Idle after 60s)

## Features

- **Activity Tracking**: Monitor what Claude is doing in real-time
  - Thinking: Brain icon
  - Read/Edit/Write: File/pencil icons
  - Bash: Terminal icon
  - Grep/Glob: Search icon
  - Idle: Sleep icon

- **Usage Metrics**: Track token consumption and costs
  - Input/output token counts
  - Cache read/write statistics
  - Estimated cost in USD

- **Configurable Display**: Customize what shows in your status bar
  - Format strings with placeholders
  - Precision control for costs
  - Icon support via Nerd Fonts

## Installation

### Prerequisites

- Nix with flakes enabled
- Waybar
- A Nerd Font (for icons)

### Building from Source

```bash
# Clone the repository
git clone https://github.com/yourusername/llm-waybar
cd llm-waybar

# Build with Nix
nix build

# Install hooks for Claude Code
./install-hooks.sh
```

The `install-hooks.sh` script will configure Claude Code to send events to the bridge.

### NixOS Configuration

Add to your NixOS configuration:

```nix
{
  environment.systemPackages = [
    (pkgs.callPackage ./path/to/llm-waybar {})
  ];
}
```

## Configuration

### Environment Variables

Configure the bridge behavior using these environment variables:

#### `LLM_BRIDGE_STATE_PATH`

Location of the state file that stores current activity and metrics.

**Default**: `/run/user/<uid>/llm_state.json`

**Example**:
```bash
export LLM_BRIDGE_STATE_PATH="$HOME/.cache/llm_state.json"
```

#### `LLM_BRIDGE_SIGNAL`

Waybar signal number to trigger refresh on state changes.

**Default**: `8`

**Example**:
```bash
export LLM_BRIDGE_SIGNAL=10
```

#### `LLM_BRIDGE_FORMAT`

Format string for the status bar display. Supports multiple placeholders and format specifiers.

**Default**: `"{activity} | ${cost:.2}"`

**Placeholders**:

| Placeholder | Description | Example Output |
|------------|-------------|----------------|
| `{model}` | Model display name | `Opus 4.5` |
| `{activity}` | Current activity | `Thinking`, `Read`, `Edit` |
| `{icon}` | Nerd Font icon for activity | (brain icon), (file icon) |
| `{cost}` | Cost with default precision (4 decimals) | `2.5161` |
| `{cost:.N}` | Cost with N decimal places | `{cost:.2}` → `2.52` |
| `{tokens}` | Total tokens (input + output) | `15651` |
| `{input_tokens}` | Input tokens only | `12450` |
| `{output_tokens}` | Output tokens only | `3201` |
| `{cache_read}` | Cache read tokens | `45000` |
| `{cache_write}` | Cache write tokens | `2100` |

**Example Formats**:

```bash
# Minimal: just activity and cost
export LLM_BRIDGE_FORMAT="{activity} | \${cost:.2}"

# With model name
export LLM_BRIDGE_FORMAT="{model} | {activity}"

# Full metrics
export LLM_BRIDGE_FORMAT="{model} | {icon} {activity} | {tokens}K | \${cost:.2}"

# Icon-only with activity
export LLM_BRIDGE_FORMAT="{icon} {activity}"

# Detailed tokens
export LLM_BRIDGE_FORMAT="{activity} | {input_tokens}in/{output_tokens}out | \${cost:.4}"
```

## Waybar Integration

### Basic Configuration

Add to your `~/.config/waybar/config`:

```json
{
  "modules-right": ["custom/llm"],
  "custom/llm": {
    "exec": "waybar-llm-bridge status",
    "return-type": "json",
    "interval": 5,
    "signal": 8,
    "format": "{}",
    "tooltip": true
  }
}
```

### Styling

Add to your `~/.config/waybar/style.css`:

```css
#custom-llm {
  padding: 0 10px;
}

#custom-llm.idle {
  color: #888;
}

#custom-llm.thinking {
  color: #f9e2af;
}

#custom-llm.tool-active {
  color: #89b4fa;
}

#custom-llm.error {
  color: #f38ba8;
}
```

### Advanced Configuration

With custom format and faster updates:

```json
{
  "custom/llm": {
    "exec": "LLM_BRIDGE_FORMAT='{model} | {icon} {activity} | ${cost:.2}' waybar-llm-bridge status",
    "return-type": "json",
    "interval": 2,
    "signal": 8,
    "format": "{}",
    "tooltip": true,
    "max-length": 50
  }
}
```

## Usage

### CLI Commands

```bash
# Display current state (JSON for waybar)
waybar-llm-bridge status

# Send an activity event
waybar-llm-bridge event --type submit          # User submitted prompt
waybar-llm-bridge event --type tool-start --tool "Read"
waybar-llm-bridge event --type tool-end
waybar-llm-bridge event --type stop             # Conversation ended

# Update from Claude's statusline (pipe JSON from stdin)
echo '{"model":{"display_name":"Opus 4.5"},"cost":{"total_cost_usd":2.51}}' | waybar-llm-bridge statusline

# Sync usage from transcript file
waybar-llm-bridge sync-usage ~/.claude/projects/abc123/transcript.jsonl
```

### Claude Code Hooks

The bridge integrates with Claude Code via hooks in `~/.claude/settings.json`:

```json
{
  "hooks": {
    "UserPromptSubmit": "waybar-llm-bridge event --type submit",
    "UserPromptStop": "waybar-llm-bridge event --type stop",
    "ToolUseStart": "waybar-llm-bridge event --type tool-start --tool \"$TOOL_NAME\"",
    "ToolUseEnd": "waybar-llm-bridge event --type tool-end",
    "StatusLine": "waybar-llm-bridge statusline"
  }
}
```

Use `./install-hooks.sh` to set these up automatically.

## Example Outputs

### Default Format

**Waybar Display**: `Thinking | $2.52`

**Tooltip**:
```
Activity: Thinking
Cost: $2.5161
```

### Full Format

**Format**: `{model} | {icon} {activity} | {tokens} | ${cost:.2}`

**Waybar Display**: `Opus 4.5 |  Edit | 15651 | $1.23`

**Tooltip**:
```
Model: Opus 4.5
Activity: Edit
Tokens: 12450 in / 3201 out
Cache: 45000 read / 2100 write
Cost: $1.2345
```

### Icon Format

**Format**: `{icon} {activity}`

**Waybar Display**:
-  `Thinking` (brain icon)
-  `Read` (file icon)
-  `Bash` (terminal icon)

## Activity Icons

The bridge uses Nerd Font icons to represent different activities:

| Activity | Icon | Unicode |
|----------|------|---------|
| Thinking |  | `\uf0517` |
| Read |  | `\uf0214` |
| Edit/Write |  | `\uf03eb` |
| Bash |  | `\uf018d` |
| Grep/Glob |  | `\uf0349` |
| Task |  | `\uf0517` |
| Idle |  | `\uf04b2` |
| Other tools |  | `\uf0327` |

## Activity Timeout

Activities automatically reset to "Idle" after 60 seconds of inactivity. This prevents stale status when Claude Code sessions end unexpectedly.

## Multi-Session Support

Run multiple Claude Code sessions and see aggregated status:

### Setup

1. Start the aggregator daemon:
```bash
waybar-llm-bridge daemon --aggregate &
```

2. Update your Claude hooks to include session ID (automatic with `install-hooks.sh`).

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

Available scenarios:
- **single-session**: Basic tool activity progression
- **multi-session**: Aggregate view from multiple sessions

## Development

### Building

```bash
# Development build
nix develop
cargo build

# Release build
nix build
```

### Testing

```bash
# Run unit tests
nix develop -c cargo test

# Run integration tests
nix build
./test-hooks.sh
```

### Project Structure

```
llm-waybar/
├── crates/
│   ├── llm-bridge-core/      # Core types and state management
│   ├── llm-bridge-claude/    # Claude-specific integrations
│   └── waybar-llm-bridge/    # Main CLI application
├── docs/
│   └── plans/                # Implementation plans
├── install-hooks.sh          # Hook installation script
├── test-hooks.sh            # Integration test suite
├── flake.nix                # Nix flake configuration
└── README.md                # This file
```

## Troubleshooting

### Waybar shows "no state"

The state file doesn't exist yet. Start a Claude Code session or manually create one:

```bash
waybar-llm-bridge event --type submit
```

### Activity not updating

Check that:
1. Claude Code hooks are installed (`./install-hooks.sh`)
2. `LLM_BRIDGE_STATE_PATH` matches in both Waybar config and hooks
3. Waybar signal number matches `LLM_BRIDGE_SIGNAL`

### Icons not displaying

Ensure you have a Nerd Font installed and configured in Waybar:

```css
#custom-llm {
  font-family: "JetBrainsMono Nerd Font";
}
```

### Format string not applied

Make sure `LLM_BRIDGE_FORMAT` is exported before running `waybar-llm-bridge status`:

```json
{
  "custom/llm": {
    "exec": "LLM_BRIDGE_FORMAT='{icon} {activity}' waybar-llm-bridge status"
  }
}
```

## License

MIT (or your chosen license)

## Contributing

Contributions welcome! Please open an issue or PR on GitHub.

## Credits

Built for use with Claude Code by Anthropic.
