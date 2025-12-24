mod aggregator;

use clap::{Parser, Subcommand, ValueEnum};
use serde::Deserialize;
use std::io::{self, BufRead, IsTerminal};
use std::path::PathBuf;
use llm_bridge_core::{Config, WaybarState, AgentPhase, signal::signal_waybar};
use llm_bridge_claude::ClaudeProvider;
use llm_bridge_core::LlmProvider;
use notify::{Watcher, RecursiveMode, Event, EventKind};
use std::sync::mpsc::channel;
use std::time::Duration;

#[derive(Parser)]
#[command(name = "waybar-llm-bridge")]
#[command(about = "Bridge LLM agents to Waybar status bar")]
struct Cli {
    #[arg(long, env = "LLM_BRIDGE_STATE_PATH")]
    state_path: Option<PathBuf>,

    #[arg(long, env = "LLM_BRIDGE_SIGNAL", default_value = "8")]
    signal: u8,

    #[arg(long, env = "LLM_BRIDGE_FORMAT")]
    format: Option<String>,

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
        #[arg(long)]
        session_id: Option<String>,
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
        /// Watch transcript file for changes
        #[arg(long)]
        log_path: Option<PathBuf>,

        /// Aggregate mode: watch sessions directory instead
        #[arg(long)]
        aggregate: bool,

        /// Sessions directory (for aggregate mode)
        #[arg(long)]
        sessions_dir: Option<PathBuf>,
    },
    /// Claude Code statusLine mode - reads JSON from stdin, outputs status line
    Statusline,
    /// Install hooks into ~/.claude/settings.json
    InstallHooks {
        /// Print what would be done without modifying the file
        #[arg(long)]
        dry_run: bool,
    },
    /// Remove hooks from ~/.claude/settings.json
    UninstallHooks {
        /// Print what would be done without modifying the file
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Clone, ValueEnum)]
enum EventType {
    Submit,
    ToolStart,
    ToolEnd,
    Stop,
}

/// JSON input from Claude Code's statusLine hook
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct StatuslineInput {
    session_id: Option<String>,
    transcript_path: Option<String>,
    cwd: Option<String>,
    model: Option<ModelInfo>,
    cost: Option<CostInfo>,
    context_window: Option<ContextWindow>,
}

#[derive(Debug, Deserialize)]
struct ModelInfo {
    id: Option<String>,
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CostInfo {
    total_cost_usd: Option<f64>,
}

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

fn main() {
    let cli = Cli::parse();
    let config = Config::from_env();
    let state_path = cli.state_path.unwrap_or(config.state_path);
    let format = cli.format.unwrap_or(config.format);

    let result = match cli.command {
        Commands::Event { r#type, tool, payload, session_id } => {
            handle_event(r#type, tool, payload, session_id, &state_path, &config.sessions_dir, cli.signal, &format)
        }
        Commands::SyncUsage { log_path } => {
            handle_sync_usage(&log_path, &state_path, cli.signal)
        }
        Commands::Status => {
            handle_status(&state_path)
        }
        Commands::Daemon { log_path, aggregate, sessions_dir } => {
            if aggregate {
                let sessions = sessions_dir.unwrap_or(config.sessions_dir);
                handle_daemon_aggregate(&sessions, &state_path, cli.signal)
            } else if let Some(log) = log_path {
                handle_daemon(&log, &state_path, cli.signal)
            } else {
                Err("Either --log-path or --aggregate is required".into())
            }
        }
        Commands::Statusline => {
            handle_statusline(&state_path, &config.sessions_dir, cli.signal, &format)
        }
        Commands::InstallHooks { dry_run } => {
            handle_install_hooks(dry_run)
        }
        Commands::UninstallHooks { dry_run } => {
            handle_uninstall_hooks(dry_run)
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
    session_id: Option<String>,
    state_path: &PathBuf,
    sessions_dir: &PathBuf,
    signal: u8,
    format: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // Read existing state to preserve data from other sources (like statusline)
    let mut state = WaybarState::read_from(state_path).unwrap_or_default();

    // Determine the phase based on event type
    let phase = match event_type {
        EventType::Submit => AgentPhase::Thinking,
        EventType::ToolStart => AgentPhase::ToolUse {
            tool: tool.unwrap_or_else(|| "unknown".to_string()),
        },
        EventType::ToolEnd => AgentPhase::Thinking,
        EventType::Stop => AgentPhase::Idle,
    };

    // Update only activity-related fields (activity, class, alt)
    // Preserve tooltip which may contain usage/cost data from statusline
    let (activity, class, alt) = match &phase {
        AgentPhase::Idle => ("Idle".to_string(), "idle".to_string(), "idle".to_string()),
        AgentPhase::Thinking => ("Thinking".to_string(), "thinking".to_string(), "active".to_string()),
        AgentPhase::ToolUse { tool } => {
            // Truncate very long tool names (>20 chars)
            let truncated = if tool.len() > 20 {
                format!("{}...", &tool[..17])
            } else {
                tool.clone()
            };
            (truncated, "tool-active".to_string(), "active".to_string())
        },
        AgentPhase::Error { message } => (format!("Error: {}", message), "error".to_string(), "error".to_string()),
    };

    state.activity = activity;
    state.class = class;
    state.alt = alt;
    // Note: tooltip is preserved from previous state (may contain cost/usage data)

    // Update last activity time (current Unix timestamp)
    state.last_activity_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    // Compute text from format string
    state.text = state.compute_text(format);

    // Set session_id if provided
    if let Some(sid) = session_id {
        state.session_id = sid;
    }

    // Write to session-specific file
    let _ = state.write_session_file(sessions_dir);

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

fn handle_statusline(
    state_path: &PathBuf,
    sessions_dir: &PathBuf,
    signal: u8,
    format: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();

    // Check if stdin is a TTY (no piped input)
    if stdin.is_terminal() {
        eprintln!("Error: statusline expects JSON input piped from Claude Code's statusLine hook.");
        eprintln!("This command is not meant to be run directly.");
        eprintln!();
        eprintln!("To install the statusLine hook, run:");
        eprintln!("  waybar-llm-bridge install-hooks");
        return Err("No input provided".into());
    }

    // Read JSON from stdin (Claude Code pipes this)
    let mut input = String::new();
    for line in stdin.lock().lines() {
        input.push_str(&line?);
    }

    // Parse the statusline input
    let status_input: StatuslineInput = serde_json::from_str(&input).unwrap_or(StatuslineInput {
        session_id: None,
        transcript_path: None,
        cwd: None,
        model: None,
        cost: None,
        context_window: None,
    });

    // Extract model name and cost from input
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

    // Output a single status line (for Claude Code's statusLine display)
    let status_line = format!("{} | ${:.2}", model_name, cost);
    println!("{}", status_line);

    // Read existing state to preserve activity field
    let mut state = WaybarState::read_from(state_path).unwrap_or_default();

    // Update model and cost fields from statusline input
    state.model = model_name.to_string();
    state.cost = cost;

    // Store session metadata
    if let Some(ref sid) = status_input.session_id {
        state.session_id = sid.clone();
    }
    if let Some(ref cwd) = status_input.cwd {
        state.cwd = cwd.clone();
    }

    // Parse transcript for token usage if transcript_path is provided
    if let Some(transcript_path) = status_input.transcript_path {
        let transcript_pathbuf = PathBuf::from(transcript_path);
        if transcript_pathbuf.exists() {
            let provider = ClaudeProvider::new();
            if let Ok(usage) = provider.parse_usage(&transcript_pathbuf) {
                // Update token fields from parsed usage
                state.input_tokens = usage.input_tokens;
                state.output_tokens = usage.output_tokens;
                state.cache_read = usage.cache_read;
                state.cache_write = usage.cache_write;
                // Note: cost from usage calculation may differ from Claude Code's reported cost
                // We prefer Claude Code's cost if available, otherwise use calculated cost
                if state.cost == 0.0 {
                    state.cost = usage.estimated_cost;
                }
            }
        }
    }

    // Preserve activity and class fields from existing state
    // (These are set by event hooks and should not be overwritten)

    // Compute text from format string
    state.text = state.compute_text(format);

    // Regenerate tooltip with all available information (including token breakdown)
    state.tooltip = state.compute_tooltip();

    // Write to session-specific file (for multi-session aggregation)
    let _ = state.write_session_file(sessions_dir);

    // Write merged state and signal waybar
    state.write_atomic(state_path)?;
    let _ = signal_waybar(signal);

    Ok(())
}

fn handle_daemon_aggregate(
    sessions_dir: &PathBuf,
    state_path: &PathBuf,
    signal: u8,
) -> Result<(), Box<dyn std::error::Error>> {
    use aggregator::SessionAggregator;

    let aggregator = SessionAggregator::new(
        sessions_dir.clone(),
        state_path.clone(),
        signal,
    );

    aggregator.watch()
}

fn handle_install_hooks(dry_run: bool) -> Result<(), Box<dyn std::error::Error>> {
    let settings_path = dirs::home_dir()
        .ok_or("Could not find home directory")?
        .join(".claude/settings.json");

    // Read existing settings or start with empty object
    let mut settings: serde_json::Value = if settings_path.exists() {
        let content = std::fs::read_to_string(&settings_path)?;
        serde_json::from_str(&content)?
    } else {
        serde_json::json!({})
    };

    // Get the binary path (use current exe or fallback to command name)
    let bin_path = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "waybar-llm-bridge".to_string());

    // Define our hooks
    let our_hooks = serde_json::json!({
        "UserPromptSubmit": [{
            "matcher": "",
            "hooks": [{
                "type": "command",
                "command": format!("{} event --type submit", bin_path)
            }]
        }],
        "PreToolUse": [{
            "matcher": "*",
            "hooks": [{
                "type": "command",
                "command": format!("{} event --type tool-start --tool \"$CLAUDE_TOOL_NAME\"", bin_path)
            }]
        }],
        "PostToolUse": [{
            "matcher": "*",
            "hooks": [{
                "type": "command",
                "command": format!("{} event --type tool-end", bin_path)
            }]
        }],
        "Stop": [{
            "matcher": "",
            "hooks": [{
                "type": "command",
                "command": format!("{} event --type stop", bin_path)
            }]
        }]
    });

    // Merge hooks into settings
    let hooks = settings
        .as_object_mut()
        .ok_or("Settings is not an object")?
        .entry("hooks")
        .or_insert(serde_json::json!({}));

    if let Some(hooks_obj) = hooks.as_object_mut() {
        for (event, hook_array) in our_hooks.as_object().unwrap() {
            let existing = hooks_obj.entry(event).or_insert(serde_json::json!([]));
            if let Some(existing_arr) = existing.as_array_mut() {
                // Remove any existing waybar-llm-bridge hooks (to update with new path)
                existing_arr.retain(|h| {
                    !h.get("hooks")
                        .and_then(|arr| arr.as_array())
                        .map(|arr| arr.iter().any(|cmd| {
                            cmd.get("command")
                                .and_then(|c| c.as_str())
                                .map(|s| s.contains("waybar-llm-bridge"))
                                .unwrap_or(false)
                        }))
                        .unwrap_or(false)
                });
                // Add our new hooks
                if let Some(new_hooks) = hook_array.as_array() {
                    for hook in new_hooks {
                        existing_arr.push(hook.clone());
                    }
                }
            }
        }
    }

    // Always update statusLine config (to handle path changes)
    let settings_obj = settings.as_object_mut().ok_or("Settings is not an object")?;
    settings_obj.insert("statusLine".to_string(), serde_json::json!({
        "type": "command",
        "command": format!("{} statusline", bin_path),
        "padding": 0
    }));

    let output = serde_json::to_string_pretty(&settings)?;

    if dry_run {
        println!("Would write to {}:\n{}", settings_path.display(), output);
    } else {
        // Create parent directory if needed
        if let Some(parent) = settings_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&settings_path, output)?;
        println!("Hooks installed to {}", settings_path.display());
    }

    Ok(())
}

fn handle_uninstall_hooks(dry_run: bool) -> Result<(), Box<dyn std::error::Error>> {
    let settings_path = dirs::home_dir()
        .ok_or("Could not find home directory")?
        .join(".claude/settings.json");

    if !settings_path.exists() {
        println!("No settings file found at {}", settings_path.display());
        return Ok(());
    }

    let content = std::fs::read_to_string(&settings_path)?;
    let mut settings: serde_json::Value = serde_json::from_str(&content)?;

    let mut removed_hooks = false;
    let mut removed_statusline = false;

    // Remove waybar-llm-bridge hooks
    if let Some(hooks) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        for (_event, hook_array) in hooks.iter_mut() {
            if let Some(arr) = hook_array.as_array_mut() {
                let before = arr.len();
                arr.retain(|h| {
                    !h.get("hooks")
                        .and_then(|arr| arr.as_array())
                        .map(|arr| arr.iter().any(|cmd| {
                            cmd.get("command")
                                .and_then(|c| c.as_str())
                                .map(|s| s.contains("waybar-llm-bridge"))
                                .unwrap_or(false)
                        }))
                        .unwrap_or(false)
                });
                if arr.len() < before {
                    removed_hooks = true;
                }
            }
        }

        // Clean up empty hook arrays
        hooks.retain(|_, v| {
            v.as_array().map(|a| !a.is_empty()).unwrap_or(true)
        });
    }

    // Remove empty hooks object
    if settings.get("hooks").and_then(|h| h.as_object()).map(|h| h.is_empty()).unwrap_or(false) {
        settings.as_object_mut().unwrap().remove("hooks");
    }

    // Remove statusLine if it's ours
    if let Some(status_line) = settings.get("statusLine") {
        if status_line.get("command")
            .and_then(|c| c.as_str())
            .map(|s| s.contains("waybar-llm-bridge"))
            .unwrap_or(false)
        {
            settings.as_object_mut().unwrap().remove("statusLine");
            removed_statusline = true;
        }
    }

    if !removed_hooks && !removed_statusline {
        println!("No waybar-llm-bridge hooks found in {}", settings_path.display());
        return Ok(());
    }

    let output = serde_json::to_string_pretty(&settings)?;

    if dry_run {
        println!("Would write to {}:\n{}", settings_path.display(), output);
        if removed_hooks {
            println!("\nWould remove: hooks");
        }
        if removed_statusline {
            println!("Would remove: statusLine");
        }
    } else {
        std::fs::write(&settings_path, output)?;
        println!("Removed waybar-llm-bridge from {}", settings_path.display());
        if removed_hooks {
            println!("  - Removed hooks");
        }
        if removed_statusline {
            println!("  - Removed statusLine");
        }
    }

    Ok(())
}
