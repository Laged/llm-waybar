//! Session aggregation for multi-session waybar display

use llm_bridge_core::{WaybarState, signal::signal_waybar};
use notify::{Watcher, RecursiveMode, Event, EventKind};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Aggregated state from multiple sessions
#[derive(Debug, Clone)]
pub struct AggregateState {
    pub text: String,
    pub tooltip: String,
    pub class: String,
    pub alt: String,
    pub sessions: usize,
    pub total_cost: f64,
}

impl Default for AggregateState {
    fn default() -> Self {
        Self {
            text: "Idle".to_string(),
            tooltip: String::new(),
            class: "idle".to_string(),
            alt: "idle".to_string(),
            sessions: 0,
            total_cost: 0.0,
        }
    }
}

/// Session aggregator that watches a directory of session files
pub struct SessionAggregator {
    sessions_dir: PathBuf,
    output_path: PathBuf,
    signal: u8,
    stale_timeout_secs: u64,
}

impl SessionAggregator {
    pub fn new(sessions_dir: PathBuf, output_path: PathBuf, signal: u8) -> Self {
        Self {
            sessions_dir,
            output_path,
            signal,
            stale_timeout_secs: 300, // 5 minutes
        }
    }

    /// Read all session files and compute aggregate state
    pub fn aggregate(&self) -> AggregateState {
        let mut sessions: Vec<WaybarState> = Vec::new();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        if let Ok(entries) = fs::read_dir(&self.sessions_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "json").unwrap_or(false) {
                    if let Ok(state) = WaybarState::read_from(&path) {
                        // Skip stale sessions
                        if state.last_activity_time > 0
                            && (now - state.last_activity_time) < self.stale_timeout_secs as i64
                        {
                            sessions.push(state);
                        }
                    }
                }
            }
        }

        self.compute_aggregate(&sessions)
    }

    fn compute_aggregate(&self, sessions: &[WaybarState]) -> AggregateState {
        if sessions.is_empty() {
            return AggregateState::default();
        }

        // Count activities by type
        let mut activity_counts: HashMap<String, usize> = HashMap::new();
        let mut total_cost = 0.0;
        let mut any_active = false;

        for session in sessions {
            let activity = &session.activity;
            *activity_counts.entry(activity.clone()).or_insert(0) += 1;
            total_cost += session.cost;
            if activity != "Idle" {
                any_active = true;
            }
        }

        // Build text with icons
        let text = self.build_aggregate_text(&activity_counts, total_cost);

        // Build tooltip with per-session breakdown
        let tooltip = self.build_aggregate_tooltip(sessions, total_cost);

        AggregateState {
            text,
            tooltip,
            class: if any_active { "active".to_string() } else { "idle".to_string() },
            alt: if any_active { "active".to_string() } else { "idle".to_string() },
            sessions: sessions.len(),
            total_cost,
        }
    }

    fn build_aggregate_text(&self, counts: &HashMap<String, usize>, total_cost: f64) -> String {
        let mut parts: Vec<String> = Vec::new();

        // Map activities to icons and counts
        let icon_map = [
            ("Thinking", "󰔟"),
            ("Read", "󰈔"),
            ("Edit", "󰏫"),
            ("Write", "󰏫"),
            ("Bash", "󰆍"),
            ("Grep", "󰍉"),
            ("Glob", "󰍉"),
            ("Task", "󰔟"),
        ];

        for (activity, icon) in icon_map {
            if let Some(&count) = counts.get(activity) {
                if count > 0 {
                    parts.push(format!("{} {}", count, icon));
                }
            }
        }

        // Handle Idle separately
        if let Some(&idle_count) = counts.get("Idle") {
            if idle_count > 0 && parts.is_empty() {
                return format!("󰒲 Idle | ${:.2}", total_cost);
            }
        }

        if parts.is_empty() {
            format!("󰒲 Idle | ${:.2}", total_cost)
        } else {
            format!("{} | ${:.2}", parts.join(" "), total_cost)
        }
    }

    fn build_aggregate_tooltip(&self, sessions: &[WaybarState], total_cost: f64) -> String {
        let mut lines = vec![
            format!("{} active sessions | ${:.2} total", sessions.len(), total_cost),
            String::new(),
        ];

        for session in sessions {
            let cwd_short = session.cwd
                .replace(dirs::home_dir().unwrap_or_default().to_str().unwrap_or(""), "~");
            lines.push(format!(
                "{}: {} - {} (${:.2})",
                cwd_short,
                session.model,
                session.activity,
                session.cost
            ));
        }

        lines.join("\n")
    }

    /// Write aggregate state to output file
    pub fn write_aggregate(&self, state: &AggregateState) -> std::io::Result<()> {
        let waybar_state = WaybarState {
            text: state.text.clone(),
            tooltip: state.tooltip.clone(),
            class: state.class.clone(),
            alt: state.alt.clone(),
            cost: state.total_cost,
            ..Default::default()
        };

        waybar_state.write_atomic(&self.output_path)
    }

    /// Clean up stale session files
    pub fn cleanup_stale(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        if let Ok(entries) = fs::read_dir(&self.sessions_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "json").unwrap_or(false) {
                    if let Ok(state) = WaybarState::read_from(&path) {
                        if state.last_activity_time > 0
                            && (now - state.last_activity_time) > self.stale_timeout_secs as i64
                        {
                            let _ = fs::remove_file(&path);
                        }
                    }
                }
            }
        }
    }

    /// Watch sessions directory and update aggregate on changes
    pub fn watch(&self) -> Result<(), Box<dyn std::error::Error>> {
        let (tx, rx) = channel();

        let mut watcher = notify::recommended_watcher(move |res: Result<Event, _>| {
            if let Ok(event) = res {
                if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)) {
                    let _ = tx.send(());
                }
            }
        })?;

        // Ensure directory exists
        fs::create_dir_all(&self.sessions_dir)?;

        watcher.watch(&self.sessions_dir, RecursiveMode::NonRecursive)?;

        eprintln!("Aggregator watching {} for session changes...", self.sessions_dir.display());

        // Initial aggregate
        let state = self.aggregate();
        self.write_aggregate(&state)?;
        let _ = signal_waybar(self.signal);

        loop {
            match rx.recv_timeout(Duration::from_secs(60)) {
                Ok(()) => {
                    // Debounce rapid changes
                    std::thread::sleep(Duration::from_millis(50));

                    // Drain any queued events
                    while rx.try_recv().is_ok() {}

                    self.cleanup_stale();
                    let state = self.aggregate();
                    let _ = self.write_aggregate(&state);
                    let _ = signal_waybar(self.signal);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    // Periodic cleanup
                    self.cleanup_stale();
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        Ok(())
    }
}
