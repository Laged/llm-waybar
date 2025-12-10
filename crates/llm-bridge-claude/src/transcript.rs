use serde::Deserialize;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use llm_bridge_core::provider::ProviderError;

#[derive(Debug, Deserialize)]
pub struct TranscriptEntry {
    #[serde(rename = "type")]
    pub entry_type: Option<String>,
    #[serde(default)]
    pub message: Option<TranscriptMessage>,
}

#[derive(Debug, Deserialize)]
pub struct TranscriptMessage {
    #[serde(default)]
    pub usage: Option<TokenUsage>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct TokenUsage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
}

pub fn parse_transcript_tail(path: &Path, max_lines: usize) -> Result<Vec<TokenUsage>, ProviderError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut usages = Vec::new();

    // Read all lines and take last max_lines
    let lines: Vec<_> = reader.lines().collect();
    let start = lines.len().saturating_sub(max_lines);

    for line_result in lines.into_iter().skip(start) {
        let line = line_result?;
        if line.trim().is_empty() {
            continue;
        }

        if let Ok(entry) = serde_json::from_str::<TranscriptEntry>(&line) {
            if let Some(message) = entry.message {
                if let Some(usage) = message.usage {
                    usages.push(usage);
                }
            }
        }
    }

    Ok(usages)
}
