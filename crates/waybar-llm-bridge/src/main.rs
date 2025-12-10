use clap::{Parser, Subcommand, ValueEnum};
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
