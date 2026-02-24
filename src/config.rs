//! Configuration loading from .env and environment variables.

use std::path::PathBuf;

/// LLM backend provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmBackend {
    Anthropic,
}

/// Application configuration.
#[derive(Debug, Clone)]
pub struct ClawedConfig {
    pub api_key: String,
    pub model: String,
    pub backend: LlmBackend,
    pub skills_dir: PathBuf,
    pub max_turns: u32,
    pub skills_enabled: bool,
}

impl ClawedConfig {
    /// Load configuration from environment variables.
    pub fn from_env() -> Result<Self, String> {
        // Try to load .env from current directory, then parent
        let _ = dotenvy::dotenv();
        let _ = dotenvy::from_filename("../.env");

        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| "ANTHROPIC_API_KEY not set. Create a .env file with your API key.")?;

        let model = std::env::var("CLAWED_MODEL")
            .unwrap_or_else(|_| "claude-sonnet-4-20250514".to_string());

        let backend = match std::env::var("CLAWED_BACKEND")
            .unwrap_or_else(|_| "anthropic".to_string())
            .to_lowercase()
            .as_str()
        {
            "anthropic" => LlmBackend::Anthropic,
            other => return Err(format!("Unknown backend: {}", other)),
        };

        let skills_dir = std::env::var("CLAWED_SKILLS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".clawed")
                    .join("skills")
            });

        let max_turns = std::env::var("CLAWED_MAX_TURNS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(50);

        Ok(Self {
            api_key,
            model,
            backend,
            skills_dir,
            max_turns,
            skills_enabled: true,
        })
    }
}
