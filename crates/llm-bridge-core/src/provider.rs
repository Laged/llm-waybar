use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProviderError {
    #[error("Failed to parse event: {0}")]
    ParseEvent(String),
    #[error("Failed to parse usage: {0}")]
    ParseUsage(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub enum LlmEvent {
    Submit { prompt: Option<String> },
    ToolStart { tool: String, input: Option<String> },
    ToolEnd { tool: String, error: Option<String> },
    Stop,
}

#[derive(Debug, Clone, Default)]
pub struct UsageMetrics {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub estimated_cost: f64,
}

pub trait LlmProvider: Send + Sync {
    fn name(&self) -> &'static str;
    fn parse_event(&self, event_type: &str, payload: Option<&str>) -> Result<LlmEvent, ProviderError>;
    fn parse_usage(&self, log_path: &Path) -> Result<UsageMetrics, ProviderError>;
}
