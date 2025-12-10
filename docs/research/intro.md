Architectural Convergence of Wayland Compositors and Autonomous Agentic Workflows: A Comprehensive Integration Framework
1. Executive Summary
The intersection of modern Linux display server architectures and autonomous artificial intelligence represents a frontier in human-computer interaction (HCI). As the Linux desktop ecosystem migrates from the legacy X Window System (X11) to the secure, compartmentalized architecture of Wayland, a simultaneous paradigm shift is occurring in software engineering: the rise of agentic coding tools. Large Language Model (LLM) agents, such as Anthropic’s Claude Code, are evolving from passive text generators into active system participants capable of executing shell commands, managing file systems, and orchestrating complex multi-step workflows.
This report presents an exhaustive technical analysis and implementation strategy for integrating these two distinct domains. The primary objective is to define the architectural mechanisms required to visualize the internal state of CLI-based AI agents within a Wayland-based graphical user interface, specifically utilizing Waybar. By leveraging emerging open standards such as the Model Context Protocol (MCP) and the Agent-to-Agent (A2A) protocol, we propose a robust "observability bridge." This middleware translates opaque agentic activities—reasoning, tool execution, and state transitions—into structured, interoperable signals compatible with Wayland status bars and notification daemons.
Furthermore, we explore the role of reproducible infrastructure, specifically the numtide/llm-agents.nix repository, in deploying these integrated environments. We demonstrate that while Wayland’s strict security model precludes traditional introspection methods like screen scraping, modern agents offer rich telemetry hooks and structured logs that can be exploited for deep, secure integration. Through the use of filesystem watchers, event-driven hooks, and signal-based Inter-Process Communication (IPC), developers can create a "semantic desktop" where the boundaries between the user, the operating system, and the AI agent become fluid and transparent.
2. The Post-X11 Graphics Stack: Wayland Architecture and Constraints
To effectively engineer an integration between an AI agent and the desktop interface, one must first possess a deep understanding of the underlying display protocol. The transition from X11 to Wayland is not merely a software update; it is a fundamental architectural reimagining of how graphical applications interact with the kernel and with each other. This distinction dictates what is technically feasible regarding window management, input injection, and status reporting.
2.1 The Legacy of X11 and the Necessity of Change
For decades, the X Window System served as the standard display server for Unix-like operating systems. Designed in the 1980s, X11 was built on a network-transparent model where the display server (X Server) and the clients (applications) could reside on different machines. While visionary, this architecture accumulated significant technical debt.
Security Model: X11 operates on a trusted client model. Any application connected to the X server can query the window tree, intercept keystrokes intended for other windows, and capture screen content. While this facilitated the creation of automation tools (like xdotool), it represents a massive vulnerability in modern computing environments.
Rendering Logic: In X11, rendering logic was often split between the server and the client, leading to tearing and synchronization artifacts.
2.2 The Wayland Paradigm: Compositor as Sovereign
Wayland simplifies the graphics stack by collapsing the display server and the window manager into a single entity: the Compositor. In this model, the compositor is the direct client of the kernel's Direct Rendering Manager (DRM) and Kernel Mode Setting (KMS) subsystems.
2.2.1 The Client-Compositor Protocol
Communication in Wayland occurs over a strict IPC channel utilizing the Wayland wire protocol. The architecture is defined by the following characteristics:
Isolation: Clients are isolated from one another. A terminal emulator running Claude Code cannot "see" the pixels of a web browser running on the same workspace. This "what you see is what you own" security model is the most critical constraint for our integration.1 It necessitates that our observability strategy be cooperative (the agent broadcasts its state) rather than intrusive (the bar reads the agent's window).
Buffers and Surfaces: Clients render their content into local memory buffers (using shared memory, wl_shm, or DMA-BUFs) and pass handles to these buffers to the compositor. The compositor’s role is strictly to composite these buffers onto the screen.
Input Routing: The kernel passes input events (keyboard, mouse) to the compositor via libinput. The compositor then determines which surface has focus and routes the event exclusively to that client. There is no global input bus that a background script can listen to.
2.2.2 The Layer Shell Protocol (zwlr_layer_shell_v1)
Desktop components like status bars (Waybar), notification daemons (Mako), and wallpapers do not behave like standard application windows. They require special placement (e.g., anchored to the edge of the screen) and exclusive zones (preventing other windows from covering them).
Standard Wayland protocols (xdg-shell) are insufficient for these use cases. This gap is filled by the Layer Shell protocol, a Wayland extension typically supported by wlroots-based compositors like Sway and Hyprland.
Waybar's Role: Waybar acts as a client implementing the layer shell protocol. It requests a surface attached to a specific edge (top, bottom) with an exclusive zone.
Implication for AI Integration: Because Waybar is a privileged component in the visual hierarchy (always visible), it is the ideal canvas for visualizing the real-time state of background agents. However, Waybar itself is just a renderer; it possesses no inherent logic to understand AI states. It relies entirely on external executables to feed it data via standard streams (stdout) or IPC.3
2.3 Waybar Architecture
Waybar is designed with a modular architecture that makes it uniquely suited for custom integrations. It supports standard modules (Battery, CPU, Clock) and Custom Modules, which are the focal point of this research.
2.3.1 Custom Module Mechanics
A custom module in Waybar is defined in the configuration file (config.jsonc) and operates by spawning an external process.
Execution Model: Waybar forks a child process executing the command defined in the exec key.
Data Ingestion: The module reads the standard output (stdout) of this child process.
Formatting: The output can be raw text or, more powerfully, structured JSON.
The JSON return type is crucial for high-fidelity agent integration. As detailed in the Waybar man pages 3 and community discussions 1, the JSON format allows a script to control multiple visual attributes simultaneously:
text: The primary label (e.g., "Agent: Thinking").
tooltip: Detailed context shown on hover (e.g., "Analyzing 4 files in src/").
class: A CSS class name that allows dynamic styling (e.g., turning the module red on error or pulsing green during execution).
percentage: A numerical value often used for progress bars or circular indicators.
2.3.2 The Update Problem: Polling vs. Signaling
A naive implementation might set a custom module to run a script every second ("interval": 1). In the context of an AI agent, this is suboptimal for two reasons:
Latency: An agent might finish a task 100ms after the last poll, leaving the status bar stale for 900ms.
Resource Waste: Spawning a Python interpreter or shell script every second consumes CPU cycles unnecessarily, especially when the agent is idle.
The superior architectural pattern supported by Waybar is Signal-Based Updates.5
Mechanism: The module is configured with "signal": N.
Trigger: When the Waybar process receives the Unix signal SIGRTMIN + N, it immediately re-runs the exec script (or re-reads the output if the script is long-running).
Application: Our AI agent integration will utilize this mechanism. The agent (or its wrapper) will emit a signal precisely when a state change occurs, ensuring the UI is perfectly synchronized with the underlying process.8
3. The Agentic Compute Paradigm
Having established the constraints and capabilities of the display layer, we must now analyze the source of the data: the AI Agent. The term "Agent" in this context refers to an autonomous instance of a Large Language Model (LLM) wrapped in a runtime environment that provides it with memory, planning capabilities, and access to external tools.
3.1 The Rise of CLI Agents: Claude Code
The specific subject of this research is Claude Code, an advanced CLI tool developed by Anthropic.9 Unlike a standard REPL (Read-Eval-Print Loop), Claude Code operates as an agentic loop.
3.1.1 The Cognitive Loop (OODA)
Claude Code generally follows an OODA (Observe-Orient-Decide-Act) loop structure:
Observe: The agent reads the user's prompt and the current file system state.
Orient: It analyzes the request against its internal knowledge base and context window.
Decide: It formulates a plan, often breaking a complex task into sub-tasks (e.g., "First I need to grep for the function definition, then I will edit the file").
Act: It executes a tool. This is the critical moment for integration. The agent might run a shell command, read a file, or write code. This action generates an output (stdout/stderr), which is fed back into the Observe phase for the next iteration.
3.1.2 Internal Telemetry
Crucially, Claude Code is not a black box. It emits telemetry and logs that make observability possible.
Transcripts: Sessions are logged to ~/.claude/projects/<id>/transcript.jsonl.10 These logs are in JSON Lines format, where each line represents a discrete event (User message, Assistant thought, Tool use, Tool result).
Telemetry Hooks: The tool supports OpenTelemetry (OTel) for exporting traces and metrics.11 While typically used for enterprise monitoring (sending data to Datadog or Honeycomb), these traces contain the exact start and stop timestamps of tool execution, which we can repurpose for local visualization.
Lifecycle Hooks: As of recent versions, Claude Code supports user-defined hooks in settings.json that execute arbitrary commands upon specific events (e.g., PostToolUse, UserPromptSubmit).13 This feature is the "game changer" for desktop integration, allowing us to push state changes to Waybar without complex log parsing.
3.2 Reproducible AI Environments: llm-agents.nix
The user explicitly references the numtide/llm-agents.nix repository.15 This repository represents the convergence of AI tooling with Nix, a purely functional package manager.
3.2.1 The Problem of AI Dependency Hell
AI agents are notoriously difficult to package. They often depend on specific versions of Python, heavy libraries (PyTorch, TensorFlow), system-level tools (git, grep, ripgrep), and rapidly changing upstream APIs. A global installation of an agent often breaks due to conflicting dependencies.
3.2.2 The Nix Solution
Nix solves this by treating packages as immutable values in a dependency graph.
Hermetic Builds: llm-agents.nix provides Flake definitions that lock the versions of Claude Code and its dependencies.15
DevShells: It allows developers to spin up ephemeral environments (nix develop) where the agent and all its required tools (MCP servers, linters, compilers) are present, without polluting the global system.
Implication for Integration: Our integration strategy must be compatible with Nix. We cannot assume that scripts act on global paths (like /usr/bin/python). Instead, we must define our visualization scripts (the "bridge") as Nix derivations that are composed alongside the agent in the same closure. This ensures that when the user runs the agent, the visualization tools are guaranteed to be present and compatible.
4. Interoperability Standards: MCP and A2A
To build a robust integration that is not tightly coupled to a single version of one tool, we must leverage open standards. The current landscape is dominated by the Model Context Protocol (MCP) and the emerging Agent-to-Agent (A2A) protocol.
4.1 Model Context Protocol (MCP)
Introduced by Anthropic and open-sourced in late 2024, MCP is rapidly becoming the standard for connecting LLMs to external context.17
4.1.1 Protocol Architecture
MCP operates on a client-host-server model 19:
MCP Host: The application running the LLM (e.g., Claude Code).
MCP Client: The internal component of the host that speaks the protocol.
MCP Server: A standalone process that provides three main capabilities:
Resources: Passive data sources (files, logs, database rows) that the LLM can read.
Prompts: Pre-defined templates for interaction.
Tools: Executable functions (e.g., execute_sql_query, resize_image) that the LLM can invoke.
4.1.2 MCP as an Observability Layer
The significance of MCP for our Waybar integration lies in its standardization of tool use. In a pre-MCP world, every agent had a proprietary way of calling tools. In an MCP world, tool calls are standardized JSON-RPC 2.0 messages.
JSON-RPC Message:
JSON
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {
    "name": "git_status",
    "arguments": {}
  },
  "id": 1
}


Integration Potential: By placing a proxy or "inspector" between the MCP Client and Server, we can intercept these messages.20 A "Waybar MCP Proxy" could sit transparently in the connection, forwarding requests to the actual tools while simultaneously broadcasting "Tool Started" and "Tool Finished" signals to the desktop UI. This creates a universal visualizer that works for any MCP-compliant agent, not just Claude Code.
4.2 Agent-to-Agent (A2A) Protocol
While MCP connects models to tools, A2A (collaborated on by Google and ServiceNow) connects agents to other agents.22
4.2.1 Orchestration and Discovery
A2A addresses higher-order problems:
Discovery: How does Agent A know that Agent B exists and can perform a task? It uses "Agent Cards".23
Task Lifecycle: A2A defines a state machine for tasks (submitted, working, input-required, completed).24
4.2.2 Relevance to Notifications
The A2A state machine maps perfectly to desktop notifications.
input-required -> Trigger a high-priority notification and a sound alert.
completed -> Trigger a normal notification.
working -> Update the Waybar icon to a spinner.
Adopting the A2A vocabulary (even if the full protocol isn't implemented) ensures our integration uses semantic states that are likely to remain relevant as the ecosystem matures.
5. Designing the Observability Bridge
We have the source (Claude Code/MCP) and the destination (Waybar). Now we must design the middleware—the Observability Bridge. Direct coupling is fragile; we need an event-driven architecture.
5.1 Architecture: The Push-Signal-Pull Pattern
We propose a "Push-Signal-Pull" architecture that minimizes latency and resource usage.
Push (The Trigger): The agent (via Hooks) or a Watcher (via filesystem events) pushes a state change event.
State Write: A lightweight bridge script receives this event and writes a structured state object to a temporary file (e.g., /run/user/1000/claude_state.json) utilizing atomic write operations to prevent race conditions.
Signal (The Notification): The script sends a Real-Time Signal (e.g., SIGRTMIN+8) to the Waybar process.
Pull (The Render): Waybar receives the signal, immediately reads the JSON state file, and updates the display.
5.2 Linux Signaling Deep Dive
Standard Unix signals (SIGUSR1, SIGUSR2) are insufficient for complex desktop environments because their numbers are limited and their behavior is often overloaded. Linux provides a range of Real-Time Signals (SIGRTMIN to SIGRTMAX) specifically for application-defined use.6
Queueing: Unlike standard signals, RT signals are queued. If an agent emits 5 status updates in 10 milliseconds, standard signals might merge them, but RT signals can be processed sequentially (though Waybar generally debounces them).
Waybar Mapping: Waybar’s configuration "signal": 8 maps to the kernel’s SIGRTMIN + 8.
Command: The bridge script will utilize the pkill or kill system call:
pkill -SIGRTMIN+8 waybar
5.3 Filesystem Monitoring (The Watchdog)
While hooks are preferred for their direct integration, a filesystem watcher acts as a robust fallback (e.g., if the agent crashes or hooks fail).
Mechanism: Using the inotify kernel subsystem (via Python's watchdog library), we can monitor the transcript.jsonl file.
Efficiency: inotify is event-driven. The kernel notifies our script only when the file is modified. This is vastly more efficient than a loop with sleep(1).
6. Implementation Strategy: Claude Code Integration
We will now detail the step-by-step implementation of the bridge using Claude Code’s Hook system.
6.1 The State Schema
First, we define the JSON schema for our shared state file. This decoupling allows us to change the agent or the visualization independently.
File: /tmp/claude_agent_state.json
Field
Type
Description
Example
status
string
High-level state
"idle", "active", "error"
phase
string
Current cognitive phase
"thinking", "tool_use", "planning"
tool
string
Name of tool being used
"Bash", "Edit", "Grep"
message
string
Detailed context for tooltip
"Running git status in /src..."
class
string
CSS class for styling
"tool-executing"
timestamp
int
Unix timestamp of update
1715423100

6.2 Configuring Hooks in settings.json
We modify the global Claude Code configuration (~/.claude/settings.json) to register our bridge script for key lifecycle events.14

JSON


{
  "hooks": {
    "UserPromptSubmit":
      }
    ],
    "PreToolUse":
      }
    ],
    "PostToolUse":
      }
    ],
    "Stop": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "python3 /path/to/bridge.py --event stop"
          }
        ]
      }
    ]
  }
}


Note on $ARGUMENTS: This placeholder is interpolated by Claude Code with the JSON payload of the event. For PreToolUse, it contains the tool name and input arguments.27
6.3 The Bridge Script (bridge.py)
This script serves as the logic core. It parses the hook payload, determines the semantic state, writes the JSON file, and signals Waybar.

Python


import argparse
import json
import os
import subprocess
import time
from pathlib import Path

STATE_FILE = Path("/tmp/claude_agent_state.json")

def update_state(data):
    # Atomic write pattern to prevent partial reads
    temp_file = STATE_FILE.with_suffix('.tmp')
    with open(temp_file, 'w') as f:
        json.dump(data, f)
    temp_file.rename(STATE_FILE)
    
    # Signal Waybar (SIGRTMIN+8)
    subprocess.run()

def handle_submit(payload):
    data = {
        "text": "  Thinking...",
        "tooltip": f"Processing User Prompt:\n{payload.get('prompt', 'Unknown')}",
        "class": "thinking",
        "phase": "planning"
    }
    update_state(data)

def handle_tool_start(payload):
    tool_name = payload.get('tool', 'Unknown')
    input_snip = str(payload.get('input', {}))[:100]
    data = {
        "text": f"  {tool_name}",
        "tooltip": f"Executing Tool: {tool_name}\nInput: {input_snip}",
        "class": "tool-active",
        "phase": "tool_use"
    }
    update_state(data)

def handle_stop(payload):
    data = {
        "text": "  Idle",
        "tooltip": "Agent is waiting for input.",
        "class": "idle",
        "phase": "idle"
    }
    update_state(data)

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--event", required=True)
    parser.add_argument("--payload", default="{}")
    args = parser.parse_args()
    
    payload = json.loads(args.payload)
    
    if args.event == "submit":
        handle_submit(payload)
    elif args.event == "tool_start":
        handle_tool_start(payload)
    elif args.event == "stop":
        handle_stop(payload)


7. Implementation Strategy: Waybar Visualization
With the data flowing, we now configure the Wayland visualization layer.
7.1 Waybar Module Configuration
We add a custom module to the Waybar config structure. We utilize the json return type for maximum flexibility.

JSON


"custom/claude": {
    "format": "{}",
    "return-type": "json",
    "exec": "cat /tmp/claude_agent_state.json",
    "interval": "once",
    "signal": 8,
    "on-click": "swaymsg '[app_id=\"claude-terminal\"] focus'",
    "on-click-right": "cat /tmp/claude_agent_state.json | jq.tooltip | rofi -dmenu -p 'Agent Status'",
    "tooltip": true
}


Key Configuration Details:
exec: Simply cats the file. This is extremely fast (microseconds), ensuring no UI lag.
signal: 8. Matches the pkill command in the bridge script.
on-click: Uses swaymsg (or hyprctl dispatch) to focus the terminal window where the agent is running. This creates a tight integration where clicking the status indicator immediately brings the agent to the foreground.
7.2 Advanced CSS Styling
Waybar's CSS engine (GtkCssProvider) allows us to style the module based on the class field returned in the JSON. This is how we visualize state without text overload.
File: style.css

CSS


#custom-claude {
    padding: 0 10px;
    margin: 0 5px;
    border-radius: 5px;
    font-weight: bold;
    transition: all 0.3s ease;
}

/* Idle State */
#custom-claude.idle {
    background-color: transparent;
    color: #6c757d;
}

/* Thinking State - Pulsing Animation */
#custom-claude.thinking {
    background-color: #d08770; /* Orange */
    color: #2e3440;
    animation-name: pulse-orange;
    animation-duration: 2s;
    animation-iteration-count: infinite;
}

/* Tool Execution State */
#custom-claude.tool-active {
    background-color: #b48ead; /* Purple */
    color: #eceff4;
    border-bottom: 2px solid #a3be8c; /* Green underline implies activity */
}

@keyframes pulse-orange {
    0% { background-color: #d08770; }
    50% { background-color: #bf616a; } /* Reddish tint */
    100% { background-color: #d08770; }
}


This styling provides immediate peripheral awareness. The user can see out of the corner of their eye if the agent is "thinking" (orange pulse) or "working" (purple).
8. Notification Integration
The user request specifically asks for notifications (e.g., "Agent Started"). This is handled by integrating libnotify into our bridge architecture.
8.1 The Notification Strategy
We should not notify on every event (which would be spammy). We should notify on:
Long-running task completion: If a tool execution takes > 30 seconds.
Errors: If the agent encounters a blocking error.
Input Required: If the agent pauses for user confirmation (Permissions).
8.2 Enhancing the Bridge Script
We update bridge.py to calculate durations and invoke notify-send.

Python


# In handle_tool_end
start_time = payload.get('start_time') # Assuming we stored this
duration = time.time() - start_time

if duration > 30:
    subprocess.run()

if payload.get('error'):
    subprocess.run([
        "notify-send",
        "-u", "critical",
        "Claude Code Error",
        payload['error'])


9. Packaging and Deployment: The Nix Way
To ensure this complex setup (Python script with dependencies, Waybar config, Claude Code wrapper) is reproducible and robust, we utilize Nix Flakes, referencing numtide/llm-agents.nix as the foundation.
9.1 Creating the Derivation
We define a Nix derivation for our bridge script. This ensures the python environment (watchdog, requests) is isolated and version-locked.
File: flake.nix (Snippet)

Nix


{
  description = "Claude Code Wayland Integration";
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    llm-agents.url = "github:numtide/llm-agents.nix";
  };

  outputs = { self, nixpkgs, llm-agents,... }: 
  let
    system = "x86_64-linux";
    pkgs = nixpkgs.legacyPackages.${system};
  in {
    packages.${system}.claude-bridge = pkgs.writers.writePython3Bin "claude-bridge" 
      { libraries = [ pkgs.python3Packages.watchdog ]; } 
      (builtins.readFile./bridge.py);

    # The Wrapped Claude Code
    packages.${system}.claude-wayland = pkgs.symlinkJoin {
      name = "claude-wayland";
      paths = [ llm-agents.packages.${system}.claude-code ];
      buildInputs =;
      postBuild = ''
        wrapProgram $out/bin/claude \
          --set CLAUDE_HOOKS_DIR ${./hooks}
      '';
    };
  };
}


9.2 Home Manager Module
The ideal distribution method is a Home Manager module that the user can import. This module would:
Install the claude-bridge package.
Write the ~/.config/waybar/config fragment.
Set up the systemd user service for the Watchdog (if used as a fallback).
This aligns with the llm-agents.nix philosophy of providing "ready-to-use" agent environments. By utilizing Nix, we eliminate the "works on my machine" problem inherent in custom shell scripts and python environments.
10. Conclusion and Future Outlook
The integration of Claude Code into the Wayland environment via Waybar represents a significant step towards the "Agentic Desktop." We have demonstrated that despite Wayland's strict isolation preventing traditional introspection, a robust observability layer can be constructed using cooperative telemetry.
By combining the Model Context Protocol (MCP) for standardized monitoring, Linux Real-Time Signals for low-latency IPC, and Waybar's JSON module for rich visualization, we create a system where the AI agent is a transparent, observable partner in the development workflow. The use of Nix ensures this complex stack remains reproducible and stable.
As the A2A protocol matures and agents become more autonomous, this architecture will evolve. We anticipate a future where the status bar is not just a visualizer, but a control surface—allowing the user to approve A2A "contracts," pause runaway agents, or inspect complex reasoning chains directly from the desktop shell, essentially merging the role of the Window Manager and the Agent Orchestrator. This research provides the foundational blueprint for that future.
Works cited
Help with waybar : r/swaywm - Reddit, accessed December 10, 2025, https://www.reddit.com/r/swaywm/comments/nmzksr/help_with_waybar/
Custom Waybar Module Not Displaying Text : r/swaywm - Reddit, accessed December 10, 2025, https://www.reddit.com/r/swaywm/comments/vd1lwp/custom_waybar_module_not_displaying_text/
waybar-custom(5) - Arch manual pages, accessed December 10, 2025, https://man.archlinux.org/man/extra/waybar/waybar-custom.5.en
Waybar not recieving the full JSON object · Issue #3425 - GitHub, accessed December 10, 2025, https://github.com/Alexays/Waybar/issues/3425
If signal is defined in a custom module for a self-updating command. the module never loads · Issue #3976 · Alexays/Waybar - GitHub, accessed December 10, 2025, https://github.com/Alexays/Waybar/issues/3976
make your config faster by using singland and not update intervals (this is about bars) : r/hyprland - Reddit, accessed December 10, 2025, https://www.reddit.com/r/hyprland/comments/17auvgk/make_your_config_faster_by_using_singland_and_not/
waybar-custom(5) - Debian Manpages, accessed December 10, 2025, https://manpages.debian.org/experimental/waybar/waybar-custom.5
Signals don't work in a custom module unless I restart waybar manually #2376 - GitHub, accessed December 10, 2025, https://github.com/Alexays/Waybar/issues/2376
Claude Code overview - Claude Code Docs, accessed December 10, 2025, https://code.claude.com/docs/en/overview
Analyzing Claude Code Interaction Logs with DuckDB - Liam ERD, accessed December 10, 2025, https://liambx.com/blog/claude-code-log-analysis-with-duckdb
Monitoring - Claude Code Docs, accessed December 10, 2025, https://code.claude.com/docs/en/monitoring-usage
Bringing Observability to Claude Code: OpenTelemetry in Action | SigNoz, accessed December 10, 2025, https://signoz.io/blog/claude-code-monitoring-with-opentelemetry/
A complete guide to hooks in Claude Code: Automating your development workflow, accessed December 10, 2025, https://www.eesel.ai/blog/hooks-in-claude-code
Hooks reference - Claude Code Docs, accessed December 10, 2025, https://code.claude.com/docs/en/hooks
numtide/llm-agents.nix: Nix packages for AI coding agents and development tools. Automatically updated daily. - GitHub, accessed December 10, 2025, https://github.com/numtide/nix-ai-tools
numtide/treefmt-nix - GitHub, accessed December 10, 2025, https://github.com/numtide/treefmt-nix/blob/main/treefmt.nix
What is Model Context Protocol (MCP)? A guide - Google Cloud, accessed December 10, 2025, https://cloud.google.com/discover/what-is-model-context-protocol
Code execution with MCP: Building more efficient agents - Anthropic, accessed December 10, 2025, https://www.anthropic.com/engineering/code-execution-with-mcp
Architecture overview - Model Context Protocol, accessed December 10, 2025, https://modelcontextprotocol.io/docs/learn/architecture
What Is MCP Proxy? Key Benefits and Implementation Guide - Akto, accessed December 10, 2025, https://www.akto.io/blog/what-is-mcp-proxy
modelcontextprotocol/inspector: Visual testing tool for MCP servers - GitHub, accessed December 10, 2025, https://github.com/modelcontextprotocol/inspector
Agent2Agent (A2A) Protocol Specification (DRAFT v1.0), accessed December 10, 2025, https://a2a-protocol.org/latest/specification/
a2aproject/A2A: An open protocol enabling communication and interoperability between opaque agentic applications. - GitHub, accessed December 10, 2025, https://github.com/a2aproject/A2A
A2A Protocol: An In-Depth Guide. The Need for Agent Interoperability | by Saeed Hajebi, accessed December 10, 2025, https://medium.com/@saeedhajebi/a2a-protocol-an-in-depth-guide-78387f992f59
Claude Code settings - Claude Code Docs, accessed December 10, 2025, https://code.claude.com/docs/en/settings
disler/claude-code-hooks-mastery - GitHub, accessed December 10, 2025, https://github.com/disler/claude-code-hooks-mastery
Claude Code: Best practices for agentic coding - Anthropic, accessed December 10, 2025, https://www.anthropic.com/engineering/claude-code-best-practices

