# High-Performance Daemon Architecture for waybar-llm-bridge

**Date:** 2025-12-24
**Status:** Approved

## Problem

Claude Code hooks are synchronous - Claude waits for them to complete. The current architecture causes seconds of delay due to:

1. **`pgrep` subprocess** - Up to 500ms timeout on every hook call
2. **Transcript parsing** - Reads entire file to get last 100 lines
3. **`fsync` on writes** - Disk flush on every state update

## Solution: Persistent Daemon with Unix Socket IPC

### Core Architecture

```
┌─────────────────┐     Unix Datagram Socket      ┌──────────────────┐
│  Claude Code    │  ────────────────────────────▶│     Daemon       │
│                 │   (fire-and-forget, <1ms)     │                  │
│  Hook fires     │                               │  • In-memory     │
│  ┌───────────┐  │                               │    state         │
│  │ Hook bin  │──┼───── send() + exit ──────────▶│  • Debounced     │
│  └───────────┘  │                               │    signaling     │
└─────────────────┘                               │  • Async I/O     │
                                                  └────────┬─────────┘
                                                           │
                              ┌─────────────────────────────┘
                              ▼
                    ┌─────────────────┐
                    │     Waybar      │◀─── SIGRTMIN+8 (batched)
                    │  custom/llm     │
                    └─────────────────┘
```

**Key Properties:**
- Hooks return in <1ms - just `sendto()` on UDP socket and exit
- No subprocess spawning - no fork, no pgrep, no waiting
- Daemon owns all I/O - state files, waybar signaling, PID caching
- Debounced signals - rapid events batched into single waybar refresh

**Socket:** `/run/user/$UID/llm-bridge.sock` (Unix datagram, SOCK_DGRAM)

**Message Format:**
```
EVENT:submit
EVENT:tool-start:Read
EVENT:tool-end
EVENT:stop
STATUS:<json-payload>
```

### Statusline Handling

Statusline is request-response (Claude pipes JSON, expects text back), but optimized:

```
┌─────────────────┐                              ┌──────────────────┐
│  Claude Code    │   stdin (JSON)               │   Statusline     │
│                 │ ─────────────────────────────▶│   Hook Binary    │
│  statusLine     │                              │                  │
│  hook fires     │   stdout (status text)       │  1. Parse JSON   │
│                 │ ◀─────────────────────────────│  2. Extract:     │
└─────────────────┘        <5ms total            │     • model      │
                                                 │     • cost       │
                                                 │     • tokens*    │
                                                 │  3. Print line   │
                                                 │  4. Send to      │
                                                 │     daemon (UDP) │
                                                 └────────┬─────────┘
                                                          │ fire-and-forget
                                                          ▼
                                                 ┌──────────────────┐
                                                 │     Daemon       │
                                                 │  • Update state  │
                                                 │  • Write file    │
                                                 │  • Signal waybar │
                                                 └──────────────────┘
```

**Key optimization:** Claude's new `context_window.current_usage` provides token counts directly:
```json
{
  "context_window": {
    "current_usage": {
      "input_tokens": 8500,
      "output_tokens": 1200,
      "cache_creation_input_tokens": 5000,
      "cache_read_input_tokens": 2000
    }
  }
}
```

**No more transcript parsing!** This eliminates the biggest I/O bottleneck.

### Signal Coalescing

During rapid tool use, Claude can fire 20+ events/second. Smart batching prevents signal storms:

```
Time ──────────────────────────────────────────────────────────────▶

Events:   ┃tool-start┃tool-end┃tool-start┃tool-end┃tool-start┃tool-end┃
          │          │        │          │        │          │        │
          ▼          ▼        ▼          ▼        ▼          ▼        ▼
State:    [update]   [update] [update]   [update] [update]   [update]
          │                                                          │
          └──────────────── debounce window (16ms) ──────────────────┘
                                    │
                                    ▼
Waybar:                      [ONE signal]
```

**Debounce Strategy:**
- Window: 16ms (~60fps, feels instant to humans)
- Behavior: After first event, wait 16ms. If more events arrive, keep waiting.
- Max delay: 50ms cap (ensures responsiveness even during sustained bursts)

**State Updates:** Immediate in-memory, debounced to disk (every 100ms or on idle)

**Daemon State:**
```rust
struct Daemon {
    state: WaybarState,           // Always current in memory
    waybar_pid: Option<i32>,      // Cached, refreshed on ESRCH
    pending_signal: bool,         // Signal needed?
    last_signal: Instant,         // For debouncing
    dirty: bool,                  // Needs disk write?
}
```

**PID Caching:**
- Cache waybar PID on first signal
- If `kill()` returns `ESRCH` (no such process), refresh via `pgrep` once
- No pgrep on hot path

### Daemon Lifecycle & Fallback

**Startup:** Systemd user service (recommended for NixOS)

**Graceful Fallback:** When daemon isn't running, hooks still work (just slower):

```
Hook tries sendto() on socket
         │
         ▼
    ┌─────────┐
    │ Success │───▶ Return immediately (<1ms)
    └─────────┘
         │
         ▼ ECONNREFUSED / ENOENT
    ┌─────────────────┐
    │ Fallback mode   │───▶ Direct write + signal (~50-100ms)
    │ (daemon not up) │     Still works, just slower
    └─────────────────┘
```

**Socket Details:**
- Type: `SOCK_DGRAM` (Unix datagram) - no connection state, just send
- Path: `/run/user/$UID/llm-bridge.sock`
- Permissions: User-only (0600)

### NixOS/Home Manager Integration

**Flake exports Home Manager module:**

```nix
homeManagerModules.default = { config, lib, pkgs, ... }: {
  options.services.llm-bridge = {
    enable = lib.mkEnableOption "LLM Waybar Bridge daemon";
    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.system}.waybar-llm-bridge;
    };
  };

  config = lib.mkIf config.services.llm-bridge.enable {
    systemd.user.services.llm-bridge = {
      Unit = {
        Description = "LLM Waybar Bridge Daemon";
        After = [ "graphical-session.target" ];
        PartOf = [ "graphical-session.target" ];
      };
      Service = {
        ExecStart = "${config.services.llm-bridge.package}/bin/waybar-llm-bridge daemon";
        Restart = "on-failure";
        RestartSec = 1;
      };
      Install.WantedBy = [ "graphical-session.target" ];
    };

    home.packages = [ config.services.llm-bridge.package ];
  };
};
```

**Usage in nixos-config:**
```nix
{
  imports = [ inputs.llm-waybar.homeManagerModules.default ];
  services.llm-bridge.enable = true;
}
```

### Error Handling

| Scenario | Handling |
|----------|----------|
| Daemon not running | Hooks fall back to direct file write + signal |
| Waybar not running | Cache "no waybar" for 5s, avoid repeated pgrep |
| Waybar restarts | Cached PID stale → ESRCH → one pgrep refresh |
| Multiple Claude sessions | Per-session state in memory, aggregated view |
| Daemon crashes | Systemd auto-restart (1s), hooks fall back |
| Race conditions | UDP atomic, file writes atomic rename |

## Implementation Tasks

### Phase 1: Core Daemon
1. Add Unix datagram socket listener to daemon mode
2. Implement message parsing (EVENT/STATUS protocol)
3. Add in-memory state management
4. Implement signal debouncing (16ms window, 50ms cap)
5. Add PID caching for waybar

### Phase 2: Client Mode
1. Add socket client to event/statusline commands
2. Implement fallback detection (ECONNREFUSED → direct mode)
3. Parse `context_window.current_usage` for tokens (skip transcript)

### Phase 3: NixOS Integration
1. Export Home Manager module from flake
2. Add systemd service definition
3. Update wrapper with socket path env var

### Phase 4: Testing
1. Unit tests for message parsing
2. Integration tests for daemon ↔ client
3. Benchmark: measure hook latency before/after
