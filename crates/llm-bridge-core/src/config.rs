use std::env;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub state_path: PathBuf,
    pub signal: u8,
    pub transcript_dir: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            state_path: default_state_path(),
            signal: 8,
            transcript_dir: default_transcript_dir(),
        }
    }
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            state_path: env::var("LLM_BRIDGE_STATE_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|_| default_state_path()),
            signal: env::var("LLM_BRIDGE_SIGNAL")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(8),
            transcript_dir: env::var("LLM_BRIDGE_TRANSCRIPT_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| default_transcript_dir()),
        }
    }
}

fn default_state_path() -> PathBuf {
    if let Ok(runtime_dir) = env::var("XDG_RUNTIME_DIR") {
        PathBuf::from(runtime_dir).join("llm_state.json")
    } else {
        PathBuf::from("/tmp/llm_state.json")
    }
}

fn default_transcript_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude/projects")
}
