use std::os::unix::net::UnixDatagram;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use std::fs;

use llm_bridge_core::{WaybarState, AgentPhase, socket::DaemonMessage};

const DEBOUNCE_MS: u64 = 16;
const MAX_DEBOUNCE_MS: u64 = 50;
const DISK_FLUSH_MS: u64 = 100;

pub struct Daemon {
    socket_path: PathBuf,
    state_path: PathBuf,
    sessions_dir: PathBuf,
    signal_num: u8,
    format: String,

    // In-memory state
    state: WaybarState,

    // Waybar PID cache
    waybar_pid: Option<i32>,
    pid_cache_time: Instant,

    // Debouncing
    pending_signal: bool,
    first_event_time: Option<Instant>,
    last_event_time: Instant,

    // Disk write batching
    dirty: bool,
    last_disk_write: Instant,
}

impl Daemon {
    pub fn new(
        socket_path: PathBuf,
        state_path: PathBuf,
        sessions_dir: PathBuf,
        signal_num: u8,
        format: String,
    ) -> Self {
        // Load existing state if available
        let state = WaybarState::read_from(&state_path).unwrap_or_default();

        Self {
            socket_path,
            state_path,
            sessions_dir,
            signal_num,
            format,
            state,
            waybar_pid: None,
            pid_cache_time: Instant::now(),
            pending_signal: false,
            first_event_time: None,
            last_event_time: Instant::now(),
            dirty: false,
            last_disk_write: Instant::now(),
        }
    }

    pub fn handle_message(&mut self, msg: DaemonMessage) {
        match msg {
            DaemonMessage::Event { event_type, tool } => {
                self.handle_event(&event_type, tool);
            }
            DaemonMessage::Status { payload } => {
                self.handle_status(&payload);
            }
        }

        self.last_event_time = Instant::now();
        if self.first_event_time.is_none() {
            self.first_event_time = Some(Instant::now());
        }
        self.pending_signal = true;
        self.dirty = true;
    }

    fn handle_event(&mut self, event_type: &str, tool: Option<String>) {
        let phase = match event_type {
            "submit" => AgentPhase::Thinking,
            "tool-start" => AgentPhase::ToolUse {
                tool: tool.unwrap_or_else(|| "unknown".to_string()),
            },
            "tool-end" => AgentPhase::Thinking,
            "stop" => AgentPhase::Idle,
            _ => return,
        };

        let (activity, class, alt) = match &phase {
            AgentPhase::Idle => ("Idle".to_string(), "idle".to_string(), "idle".to_string()),
            AgentPhase::Thinking => ("Thinking".to_string(), "thinking".to_string(), "active".to_string()),
            AgentPhase::ToolUse { tool } => {
                // Use char count for UTF-8 safety (avoid slicing mid-character)
                let truncated = if tool.chars().count() > 20 {
                    let s: String = tool.chars().take(17).collect();
                    format!("{}...", s)
                } else {
                    tool.clone()
                };
                (truncated, "tool-active".to_string(), "active".to_string())
            }
            AgentPhase::Error { message } => {
                (format!("Error: {}", message), "error".to_string(), "error".to_string())
            }
        };

        self.state.activity = activity;
        self.state.class = class;
        self.state.alt = alt;
        self.state.last_activity_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_secs() as i64;
        self.state.text = self.state.compute_text(&self.format);
    }

    fn handle_status(&mut self, payload: &str) {
        // Parse the status payload
        #[derive(serde::Deserialize)]
        struct StatusPayload {
            session_id: Option<String>,
            cwd: Option<String>,
            model: Option<ModelInfo>,
            cost: Option<CostInfo>,
            context_window: Option<ContextWindow>,
        }

        #[derive(serde::Deserialize)]
        struct ModelInfo {
            id: Option<String>,
            display_name: Option<String>,
        }

        #[derive(serde::Deserialize)]
        struct CostInfo {
            total_cost_usd: Option<f64>,
        }

        #[derive(serde::Deserialize)]
        struct ContextWindow {
            current_usage: Option<CurrentUsage>,
        }

        #[derive(serde::Deserialize)]
        struct CurrentUsage {
            input_tokens: Option<u64>,
            output_tokens: Option<u64>,
            cache_creation_input_tokens: Option<u64>,
            cache_read_input_tokens: Option<u64>,
        }

        if let Ok(status) = serde_json::from_str::<StatusPayload>(payload) {
            if let Some(model) = status.model {
                self.state.model = model.display_name
                    .or(model.id)
                    .unwrap_or_else(|| "Claude".to_string());
            }

            if let Some(cost) = status.cost {
                self.state.cost = cost.total_cost_usd.unwrap_or(0.0);
            }

            if let Some(sid) = status.session_id {
                self.state.session_id = sid;
            }

            if let Some(cwd) = status.cwd {
                self.state.cwd = cwd;
            }

            if let Some(cw) = status.context_window {
                if let Some(usage) = cw.current_usage {
                    self.state.input_tokens = usage.input_tokens.unwrap_or(0);
                    self.state.output_tokens = usage.output_tokens.unwrap_or(0);
                    self.state.cache_read = usage.cache_read_input_tokens.unwrap_or(0);
                    self.state.cache_write = usage.cache_creation_input_tokens.unwrap_or(0);
                }
            }

            self.state.text = self.state.compute_text(&self.format);
            self.state.tooltip = self.state.compute_tooltip();
        }
    }

    /// Check if we should signal waybar (debounce logic)
    pub fn should_signal(&self) -> bool {
        if !self.pending_signal {
            return false;
        }

        let now = Instant::now();
        let since_last = now.duration_since(self.last_event_time);
        let since_first = self.first_event_time
            .map(|t| now.duration_since(t))
            .unwrap_or(Duration::ZERO);

        // Signal if: debounce window passed OR max delay reached
        since_last >= Duration::from_millis(DEBOUNCE_MS)
            || since_first >= Duration::from_millis(MAX_DEBOUNCE_MS)
    }

    /// Signal waybar and reset debounce state
    pub fn do_signal(&mut self) {
        if let Some(pid) = self.waybar_pid {
            // Try cached PID first
            if self.signal_pid(pid).is_err() {
                // PID stale, refresh
                self.refresh_waybar_pid();
                if let Some(new_pid) = self.waybar_pid {
                    let _ = self.signal_pid(new_pid);
                }
            }
        } else {
            self.refresh_waybar_pid();
            if let Some(pid) = self.waybar_pid {
                let _ = self.signal_pid(pid);
            }
        }

        self.pending_signal = false;
        self.first_event_time = None;
    }

    fn signal_pid(&self, pid: i32) -> Result<(), nix::errno::Errno> {
        use nix::sys::signal::{self, Signal};
        use nix::unistd::Pid;
        use nix::libc;

        let sig = Signal::try_from(libc::SIGRTMIN() + self.signal_num as i32)
            .map_err(|_| nix::errno::Errno::EINVAL)?;
        signal::kill(Pid::from_raw(pid), sig)
    }

    fn refresh_waybar_pid(&mut self) {
        use std::process::Command;

        if let Ok(output) = Command::new("pgrep").arg("-x").arg("waybar").output() {
            if output.status.success() {
                if let Ok(s) = String::from_utf8(output.stdout) {
                    if let Some(pid) = s.lines().next().and_then(|l| l.trim().parse().ok()) {
                        self.waybar_pid = Some(pid);
                        self.pid_cache_time = Instant::now();
                        return;
                    }
                }
            }
        }
        self.waybar_pid = None;
    }

    /// Check if we should flush to disk
    pub fn should_flush(&self) -> bool {
        self.dirty && self.last_disk_write.elapsed() >= Duration::from_millis(DISK_FLUSH_MS)
    }

    /// Flush state to disk
    pub fn do_flush(&mut self) {
        let _ = self.state.write_session_file(&self.sessions_dir);
        let _ = self.state.write_atomic(&self.state_path);
        self.dirty = false;
        self.last_disk_write = Instant::now();
    }

    /// Bind and return the socket
    pub fn bind_socket(&self) -> std::io::Result<UnixDatagram> {
        // Remove old socket if exists
        let _ = fs::remove_file(&self.socket_path);

        let socket = UnixDatagram::bind(&self.socket_path)?;
        socket.set_nonblocking(true)?;

        // Set permissions to user-only
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            let _ = fs::set_permissions(&self.socket_path, perms);
        }

        Ok(socket)
    }

    /// Main daemon loop
    pub fn run(&mut self) -> std::io::Result<()> {
        let socket = self.bind_socket()?;

        eprintln!("llm-bridge daemon listening on {:?}", self.socket_path);

        let mut buf = [0u8; 65536];

        loop {
            // Try to receive a message (non-blocking)
            match socket.recv(&mut buf) {
                Ok(n) => {
                    if let Ok(s) = std::str::from_utf8(&buf[..n]) {
                        if let Some(msg) = DaemonMessage::decode(s) {
                            self.handle_message(msg);
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No message available, that's fine
                }
                Err(e) => {
                    eprintln!("Socket error: {}", e);
                }
            }

            // Check debounce timer and signal if ready
            if self.should_signal() {
                self.do_signal();
            }

            // Check disk flush timer
            if self.should_flush() {
                self.do_flush();
            }

            // Small sleep to prevent busy-waiting
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
