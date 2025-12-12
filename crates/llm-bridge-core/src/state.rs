use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::Path;

use crate::provider::UsageMetrics;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaybarState {
    // Separate concerns - each field updated independently
    #[serde(default)]
    pub model: String,           // Set by statusline
    #[serde(default)]
    pub activity: String,        // Set by events (Idle, Thinking, Read, Edit, etc.)
    #[serde(default)]
    pub cost: f64,               // Set by statusline
    #[serde(default)]
    pub input_tokens: u64,       // Set by statusline (if available)
    #[serde(default)]
    pub output_tokens: u64,      // Set by statusline (if available)
    #[serde(default)]
    pub cache_read: u64,         // Set by statusline (if available)
    #[serde(default)]
    pub cache_write: u64,        // Set by statusline (if available)
    #[serde(default)]
    pub last_activity_time: i64, // Unix timestamp of last activity update

    // Computed from above based on format string
    #[serde(default)]
    pub text: String,            // Computed on read
    #[serde(default)]
    pub tooltip: String,
    #[serde(default)]
    pub class: String,
    #[serde(default)]
    pub alt: String,
    #[serde(default)]
    pub percentage: u8,
}

impl Default for WaybarState {
    fn default() -> Self {
        Self {
            model: String::new(),
            activity: "Idle".to_string(),
            cost: 0.0,
            input_tokens: 0,
            output_tokens: 0,
            cache_read: 0,
            cache_write: 0,
            last_activity_time: 0,
            text: "Idle".to_string(),
            tooltip: String::new(),
            class: "idle".to_string(),
            alt: "idle".to_string(),
            percentage: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentPhase {
    Idle,
    Thinking,
    ToolUse { tool: String },
    Error { message: String },
}

impl WaybarState {
    /// Get Nerd Font icon for current activity
    /// Maps activity states to appropriate icons
    pub fn get_activity_icon(&self) -> &str {
        match self.activity.as_str() {
            "Thinking" | "Thinking..." => "\u{f0517}", // 󰔟 brain
            "Read" => "\u{f0214}", // 󰈔 file
            "Edit" => "\u{f03eb}", // 󰏫 pencil
            "Write" => "\u{f03eb}", // 󰏫 pencil
            "Bash" => "\u{f018d}", // 󰆍 terminal
            "Grep" | "Glob" => "\u{f0349}", // 󰍉 search
            "Task" => "\u{f0517}", // 󰔟 agent/task
            "Idle" => "\u{f04b2}", // 󰒲 sleep
            _ => "\u{f0327}", // 󰌧 tool (default for other tools)
        }
    }

    /// Check if activity has timed out (60 seconds) and reset to Idle if needed
    /// Returns true if activity was reset
    pub fn check_activity_timeout(&mut self) -> bool {
        const TIMEOUT_SECS: i64 = 60;

        if self.activity == "Idle" {
            return false; // Already idle, no timeout
        }

        if self.last_activity_time == 0 {
            return false; // No timestamp set yet
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        if now - self.last_activity_time > TIMEOUT_SECS {
            self.activity = "Idle".to_string();
            self.class = "idle".to_string();
            self.alt = "idle".to_string();
            return true;
        }

        false
    }

    /// Compute text field from format string with placeholder replacement
    /// Supported placeholders:
    /// - {model} - model name
    /// - {activity} - current activity
    /// - {icon} - Nerd Font icon for current activity
    /// - {cost} or {cost:.N} - cost with optional precision
    /// - {tokens} - total tokens (input + output)
    /// - {input_tokens} - input tokens
    /// - {output_tokens} - output tokens
    /// - {cache_read} - cache read tokens
    /// - {cache_write} - cache write tokens
    pub fn compute_text(&self, format: &str) -> String {
        let mut result = format.to_string();

        // Replace model
        result = result.replace("{model}", &self.model);

        // Replace activity
        result = result.replace("{activity}", &self.activity);

        // Replace icon
        result = result.replace("{icon}", self.get_activity_icon());

        // Replace cost with various precisions
        // Handle {cost:.2}, {cost:.4}, etc.
        for precision in [0, 1, 2, 3, 4, 5, 6] {
            let placeholder = format!("{{cost:.{}}}", precision);
            if result.contains(&placeholder) {
                result = result.replace(&placeholder, &format!("{:.prec$}", self.cost, prec = precision));
            }
        }
        // Handle plain {cost} (default precision 4)
        result = result.replace("{cost}", &format!("{:.4}", self.cost));

        // Replace token counts
        let total_tokens = self.input_tokens + self.output_tokens;
        result = result.replace("{tokens}", &total_tokens.to_string());
        result = result.replace("{input_tokens}", &self.input_tokens.to_string());
        result = result.replace("{output_tokens}", &self.output_tokens.to_string());
        result = result.replace("{cache_read}", &self.cache_read.to_string());
        result = result.replace("{cache_write}", &self.cache_write.to_string());

        result
    }

    /// Compute detailed tooltip with all available information
    pub fn compute_tooltip(&self) -> String {
        let mut parts = Vec::new();

        if !self.model.is_empty() {
            parts.push(format!("Model: {}", self.model));
        }

        if !self.activity.is_empty() {
            parts.push(format!("Activity: {}", self.activity));
        }

        if self.input_tokens > 0 || self.output_tokens > 0 {
            parts.push(format!(
                "Tokens: {} in / {} out",
                self.input_tokens, self.output_tokens
            ));
        }

        if self.cache_read > 0 || self.cache_write > 0 {
            parts.push(format!(
                "Cache: {} read / {} write",
                self.cache_read, self.cache_write
            ));
        }

        if self.cost > 0.0 {
            parts.push(format!("Cost: ${:.4}", self.cost));
        }

        parts.join("\n")
    }

    /// Create state from agent phase, setting the activity field
    pub fn from_phase(phase: &AgentPhase, usage: Option<&UsageMetrics>) -> Self {
        let (activity, class, alt) = match phase {
            AgentPhase::Idle => ("Idle".to_string(), "idle".to_string(), "idle".to_string()),
            AgentPhase::Thinking => ("Thinking".to_string(), "thinking".to_string(), "active".to_string()),
            AgentPhase::ToolUse { tool } => (tool.clone(), "tool-active".to_string(), "active".to_string()),
            AgentPhase::Error { message } => (format!("Error: {}", message), "error".to_string(), "error".to_string()),
        };

        let mut state = Self {
            activity: activity.clone(),
            class,
            alt,
            text: activity, // Default text is activity
            ..Default::default()
        };

        // Update usage metrics if provided
        if let Some(u) = usage {
            state.input_tokens = u.input_tokens;
            state.output_tokens = u.output_tokens;
            state.cache_read = u.cache_read;
            state.cache_write = u.cache_write;
            state.cost = u.estimated_cost;
            state.tooltip = state.compute_tooltip();
        }

        state
    }

    pub fn write_atomic(&self, path: &Path) -> std::io::Result<()> {
        let tmp_path = path.with_extension("tmp");
        let json = serde_json::to_string(self)?;

        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;

        fs::rename(&tmp_path, path)?;
        Ok(())
    }

    pub fn read_from(path: &Path) -> std::io::Result<Self> {
        let content = fs::read_to_string(path)?;
        let mut state: Self = serde_json::from_str(&content)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        // Check for activity timeout and reset to Idle if needed
        state.check_activity_timeout();

        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_state_is_correct() {
        let state = WaybarState::default();

        assert_eq!(state.model, "");
        assert_eq!(state.activity, "Idle");
        assert_eq!(state.cost, 0.0);
        assert_eq!(state.input_tokens, 0);
        assert_eq!(state.output_tokens, 0);
        assert_eq!(state.cache_read, 0);
        assert_eq!(state.cache_write, 0);
        assert_eq!(state.text, "Idle");
        assert_eq!(state.class, "idle");
        assert_eq!(state.alt, "idle");
        assert_eq!(state.percentage, 0);
    }

    #[test]
    fn test_compute_text_with_model_and_activity() {
        let state = WaybarState {
            model: "Opus 4.5".to_string(),
            activity: "Thinking".to_string(),
            ..Default::default()
        };

        let result = state.compute_text("{model} | {activity}");
        assert_eq!(result, "Opus 4.5 | Thinking");
    }

    #[test]
    fn test_compute_text_with_cost_precision() {
        let state = WaybarState {
            cost: 2.51609,
            ..Default::default()
        };

        // Test different precisions
        assert_eq!(state.compute_text("${cost:.2}"), "$2.52");
        assert_eq!(state.compute_text("${cost:.4}"), "$2.5161");
        assert_eq!(state.compute_text("${cost:.0}"), "$3");

        // Test default precision (4 digits)
        assert_eq!(state.compute_text("${cost}"), "$2.5161");
    }

    #[test]
    fn test_compute_text_with_tokens() {
        let state = WaybarState {
            input_tokens: 12450,
            output_tokens: 3201,
            cache_read: 45000,
            cache_write: 2100,
            ..Default::default()
        };

        let result = state.compute_text("{tokens} total ({input_tokens} in / {output_tokens} out)");
        assert_eq!(result, "15651 total (12450 in / 3201 out)");

        let result = state.compute_text("Cache: {cache_read}r / {cache_write}w");
        assert_eq!(result, "Cache: 45000r / 2100w");
    }

    #[test]
    fn test_compute_text_with_icon() {
        let state = WaybarState {
            activity: "Thinking".to_string(),
            ..Default::default()
        };

        let result = state.compute_text("{icon} {activity}");
        assert_eq!(result, "\u{f0517} Thinking"); // brain icon + Thinking
    }

    #[test]
    fn test_compute_text_complex_format() {
        let state = WaybarState {
            model: "Opus 4.5".to_string(),
            activity: "Edit".to_string(),
            cost: 1.2345,
            input_tokens: 10000,
            output_tokens: 5000,
            ..Default::default()
        };

        let result = state.compute_text("{model} | {icon} {activity} | {tokens}K | ${cost:.2}");
        assert_eq!(result, "Opus 4.5 | \u{f03eb} Edit | 15000K | $1.23");
    }

    #[test]
    fn test_get_activity_icon_returns_correct_icons() {
        let test_cases = vec![
            ("Thinking", "\u{f0517}"),      // brain
            ("Thinking...", "\u{f0517}"),   // brain
            ("Read", "\u{f0214}"),          // file
            ("Edit", "\u{f03eb}"),          // pencil
            ("Write", "\u{f03eb}"),         // pencil
            ("Bash", "\u{f018d}"),          // terminal
            ("Grep", "\u{f0349}"),          // search
            ("Glob", "\u{f0349}"),          // search
            ("Task", "\u{f0517}"),          // agent
            ("Idle", "\u{f04b2}"),          // sleep
            ("UnknownTool", "\u{f0327}"),   // default tool icon
        ];

        for (activity, expected_icon) in test_cases {
            let state = WaybarState {
                activity: activity.to_string(),
                ..Default::default()
            };
            assert_eq!(
                state.get_activity_icon(),
                expected_icon,
                "Failed for activity: {}",
                activity
            );
        }
    }

    #[test]
    fn test_check_activity_timeout_when_idle() {
        let mut state = WaybarState {
            activity: "Idle".to_string(),
            last_activity_time: 1000,
            ..Default::default()
        };

        // Should not timeout if already idle
        let result = state.check_activity_timeout();
        assert_eq!(result, false);
        assert_eq!(state.activity, "Idle");
    }

    #[test]
    fn test_check_activity_timeout_when_no_timestamp() {
        let mut state = WaybarState {
            activity: "Thinking".to_string(),
            last_activity_time: 0, // No timestamp set
            ..Default::default()
        };

        // Should not timeout if no timestamp
        let result = state.check_activity_timeout();
        assert_eq!(result, false);
        assert_eq!(state.activity, "Thinking");
    }

    #[test]
    fn test_check_activity_timeout_recent_activity() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let mut state = WaybarState {
            activity: "Read".to_string(),
            class: "tool-active".to_string(),
            alt: "active".to_string(),
            last_activity_time: now - 30, // 30 seconds ago
            ..Default::default()
        };

        // Should not timeout (within 60 seconds)
        let result = state.check_activity_timeout();
        assert_eq!(result, false);
        assert_eq!(state.activity, "Read");
        assert_eq!(state.class, "tool-active");
    }

    #[test]
    fn test_check_activity_timeout_old_activity() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let mut state = WaybarState {
            activity: "Edit".to_string(),
            class: "tool-active".to_string(),
            alt: "active".to_string(),
            last_activity_time: now - 120, // 120 seconds ago (> 60s timeout)
            ..Default::default()
        };

        // Should timeout and reset to Idle
        let result = state.check_activity_timeout();
        assert_eq!(result, true);
        assert_eq!(state.activity, "Idle");
        assert_eq!(state.class, "idle");
        assert_eq!(state.alt, "idle");
    }

    #[test]
    fn test_compute_tooltip_with_all_data() {
        let state = WaybarState {
            model: "Opus 4.5".to_string(),
            activity: "Thinking".to_string(),
            cost: 2.5161,
            input_tokens: 12450,
            output_tokens: 3201,
            cache_read: 45000,
            cache_write: 2100,
            ..Default::default()
        };

        let tooltip = state.compute_tooltip();

        assert!(tooltip.contains("Model: Opus 4.5"));
        assert!(tooltip.contains("Activity: Thinking"));
        assert!(tooltip.contains("Tokens: 12450 in / 3201 out"));
        assert!(tooltip.contains("Cache: 45000 read / 2100 write"));
        assert!(tooltip.contains("Cost: $2.5161"));
    }

    #[test]
    fn test_compute_tooltip_minimal_data() {
        let state = WaybarState {
            activity: "Idle".to_string(),
            ..Default::default()
        };

        let tooltip = state.compute_tooltip();

        // Should only show activity when no other data is present
        assert_eq!(tooltip, "Activity: Idle");
    }

    #[test]
    fn test_from_phase_idle() {
        let state = WaybarState::from_phase(&AgentPhase::Idle, None);

        assert_eq!(state.activity, "Idle");
        assert_eq!(state.text, "Idle");
        assert_eq!(state.class, "idle");
        assert_eq!(state.alt, "idle");
    }

    #[test]
    fn test_from_phase_thinking() {
        let state = WaybarState::from_phase(&AgentPhase::Thinking, None);

        assert_eq!(state.activity, "Thinking");
        assert_eq!(state.text, "Thinking");
        assert_eq!(state.class, "thinking");
        assert_eq!(state.alt, "active");
    }

    #[test]
    fn test_from_phase_tool_use() {
        let state = WaybarState::from_phase(
            &AgentPhase::ToolUse { tool: "Read".to_string() },
            None
        );

        assert_eq!(state.activity, "Read");
        assert_eq!(state.text, "Read");
        assert_eq!(state.class, "tool-active");
        assert_eq!(state.alt, "active");
    }

    #[test]
    fn test_from_phase_error() {
        let state = WaybarState::from_phase(
            &AgentPhase::Error { message: "Connection failed".to_string() },
            None
        );

        assert_eq!(state.activity, "Error: Connection failed");
        assert_eq!(state.class, "error");
        assert_eq!(state.alt, "error");
    }

    #[test]
    fn test_from_phase_with_usage() {
        let usage = UsageMetrics {
            input_tokens: 1000,
            output_tokens: 500,
            cache_read: 2000,
            cache_write: 100,
            estimated_cost: 0.25,
        };

        let state = WaybarState::from_phase(&AgentPhase::Thinking, Some(&usage));

        assert_eq!(state.input_tokens, 1000);
        assert_eq!(state.output_tokens, 500);
        assert_eq!(state.cache_read, 2000);
        assert_eq!(state.cache_write, 100);
        assert_eq!(state.cost, 0.25);
        assert!(state.tooltip.contains("Tokens: 1000 in / 500 out"));
    }
}
