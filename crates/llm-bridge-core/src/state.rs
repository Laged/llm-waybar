use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::Path;

use crate::provider::UsageMetrics;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WaybarState {
    pub text: String,
    pub tooltip: String,
    pub class: String,
    pub alt: String,
    pub percentage: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentPhase {
    Idle,
    Thinking,
    ToolUse { tool: String },
    Error { message: String },
}

impl WaybarState {
    pub fn from_phase(phase: &AgentPhase, usage: Option<&UsageMetrics>) -> Self {
        let (text, class, alt) = match phase {
            AgentPhase::Idle => ("Idle".to_string(), "idle".to_string(), "idle".to_string()),
            AgentPhase::Thinking => ("Thinking...".to_string(), "thinking".to_string(), "active".to_string()),
            AgentPhase::ToolUse { tool } => (tool.clone(), "tool-active".to_string(), "active".to_string()),
            AgentPhase::Error { message } => (format!("Error: {}", message), "error".to_string(), "error".to_string()),
        };

        let tooltip = usage
            .map(|u| format!(
                "Tokens: {} in / {} out\nCost: ${:.4}",
                u.input_tokens, u.output_tokens, u.estimated_cost
            ))
            .unwrap_or_default();

        Self {
            text,
            tooltip,
            class,
            alt,
            percentage: 0,
        }
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
        serde_json::from_str(&content).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}
