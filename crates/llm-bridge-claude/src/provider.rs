use std::path::Path;
use llm_bridge_core::provider::{LlmProvider, LlmEvent, UsageMetrics, ProviderError};
use crate::hooks::ClaudeHookPayload;
use crate::transcript::parse_transcript_tail;
use crate::usage::calculate_cost;

pub struct ClaudeProvider;

impl ClaudeProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ClaudeProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmProvider for ClaudeProvider {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn parse_event(&self, event_type: &str, payload: Option<&str>) -> Result<LlmEvent, ProviderError> {
        let hook_payload = payload
            .map(ClaudeHookPayload::from_json)
            .transpose()
            .map_err(|e| ProviderError::ParseEvent(e.to_string()))?
            .unwrap_or_default();

        match event_type {
            "submit" => Ok(LlmEvent::Submit {
                prompt: hook_payload.prompt,
            }),
            "tool-start" => Ok(LlmEvent::ToolStart {
                tool: hook_payload.tool_name.unwrap_or_else(|| "unknown".to_string()),
                input: hook_payload.tool_input.map(|v| v.to_string()),
            }),
            "tool-end" => Ok(LlmEvent::ToolEnd {
                tool: hook_payload.tool_name.unwrap_or_else(|| "unknown".to_string()),
                error: hook_payload.error,
            }),
            "stop" => Ok(LlmEvent::Stop),
            other => Err(ProviderError::ParseEvent(format!("Unknown event type: {}", other))),
        }
    }

    fn parse_usage(&self, log_path: &Path) -> Result<UsageMetrics, ProviderError> {
        let entries = parse_transcript_tail(log_path, 100)?;
        Ok(calculate_cost(&entries))
    }
}
