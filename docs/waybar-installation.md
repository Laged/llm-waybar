# Waybar Integration for llm-waybar

This guide explains how to integrate `waybar-llm-bridge` into your NixOS Waybar configuration.

## Overview

The bridge uses a "Push-Signal-Pull" pattern:
1. Claude Code hooks **push** events to the bridge
2. Bridge updates JSON state and **signals** Waybar (SIGRTMIN+8)
3. Waybar **pulls** from the state file for display

## NixOS Configuration

### 1. Add the flake input

```nix
# flake.nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    llm-waybar.url = "github:laged/llm-waybar";  # or path:/home/laged/Codings/laged/llm-waybar
  };
}
```

### 2. Add the package

```nix
# In your configuration or home-manager
{ inputs, pkgs, ... }:
{
  environment.systemPackages = [
    inputs.llm-waybar.packages.${pkgs.system}.default
  ];

  # Or with home-manager:
  home.packages = [
    inputs.llm-waybar.packages.${pkgs.system}.default
  ];
}
```

### 3. Configure Waybar module

Add the custom module to your Waybar configuration:

```nix
# waybar config (home-manager example)
programs.waybar = {
  enable = true;
  settings = {
    mainBar = {
      # Add to your modules list
      modules-right = [ "custom/llm" /* ...other modules... */ ];

      "custom/llm" = {
        exec = "waybar-llm-bridge status";
        return-type = "json";
        interval = "once";
        signal = 8;  # SIGRTMIN+8
        format = "{}";
        tooltip = true;
      };
    };
  };

  style = ''
    /* LLM status styling */
    #custom-llm {
      padding: 0 10px;
      font-family: "JetBrainsMono Nerd Font", monospace;
    }

    #custom-llm.idle {
      color: #6c7086;  /* gray - inactive */
    }

    #custom-llm.thinking {
      color: #f9e2af;  /* yellow - processing */
    }

    #custom-llm.tool-active {
      color: #a6e3a1;  /* green - executing tool */
    }

    #custom-llm.active {
      color: #89b4fa;  /* blue - active session */
    }

    #custom-llm.error {
      color: #f38ba8;  /* red - error state */
    }
  '';
};
```

### 4. Install Claude Code hooks

After rebuilding your NixOS configuration, install the hooks:

```bash
waybar-llm-bridge install-hooks
```

This adds the necessary hooks to `~/.claude/settings.json`.

## Environment Variables

The bridge uses these environment variables (with sensible defaults):

| Variable | Default | Description |
|----------|---------|-------------|
| `LLM_BRIDGE_STATE_PATH` | `/run/user/$UID/llm_state.json` | State file location |
| `LLM_BRIDGE_SIGNAL` | `8` | Signal number (SIGRTMIN+N) |
| `LLM_BRIDGE_TRANSCRIPT_DIR` | `~/.claude/projects` | Claude transcript directory |

The Nix package sets these defaults via `wrapProgram`.

## State File Output

The bridge outputs JSON compatible with Waybar's custom module format:

```json
{
  "text": "Read",
  "tooltip": "Session cost: $0.0234",
  "class": "tool-active",
  "alt": "active",
  "percentage": 0
}
```

### Classes and their meanings

| Class | Meaning |
|-------|---------|
| `idle` | No active Claude session |
| `thinking` | Claude is processing/generating |
| `tool-active` | Claude is executing a tool (text shows tool name) |
| `active` | Active session, between operations |
| `error` | An error occurred |

## Manual Testing

Test the integration:

```bash
# Check current state
waybar-llm-bridge status

# Simulate events manually
waybar-llm-bridge event --type submit
waybar-llm-bridge event --type tool-start --tool "Read"
waybar-llm-bridge event --type tool-end
waybar-llm-bridge event --type stop

# Watch state changes
watch -n 0.2 'cat /run/user/$(id -u)/llm_state.json'
```

## Troubleshooting

### Module not updating

1. Check signal number matches: Waybar `signal = 8` should match `LLM_BRIDGE_SIGNAL=8`
2. Verify hooks are installed: `cat ~/.claude/settings.json | grep waybar`
3. Check state file exists: `cat /run/user/$(id -u)/llm_state.json`

### Waybar not receiving signals

Reload Waybar to pick up the new module:

```bash
killall -SIGUSR2 waybar
# or restart completely
systemctl --user restart waybar
```

### Wrong binary path

If using the flake, the binary is in PATH. If using a local build:

```nix
"custom/llm" = {
  exec = "/path/to/llm-waybar/result/bin/waybar-llm-bridge status";
  # ...
};
```
