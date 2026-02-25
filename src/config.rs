//! Configuration loading from .env and environment variables.

use std::path::PathBuf;

/// LLM backend provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmBackend {
    Anthropic,
    OpenAi,
    Gemini,
    ClaudeCli,
}

/// Application configuration.
#[derive(Debug, Clone)]
pub struct ClawedConfig {
    pub api_key: String,
    pub model: String,
    pub backend: LlmBackend,
    pub openai_api_key: Option<String>,
    pub openai_model: String,
    pub gemini_api_key: Option<String>,
    pub gemini_model: String,
    pub claude_cli_model: String,
    pub claude_cli_timeout_secs: u64,
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

        let backend = match std::env::var("CLAWED_BACKEND")
            .unwrap_or_else(|_| "anthropic".to_string())
            .to_lowercase()
            .as_str()
        {
            "anthropic" => LlmBackend::Anthropic,
            "openai" => LlmBackend::OpenAi,
            "gemini" => LlmBackend::Gemini,
            "claude_cli" => LlmBackend::ClaudeCli,
            other => return Err(format!("Unknown backend: {}", other)),
        };

        // Anthropic API key — required when backend is anthropic
        let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_default();
        if backend == LlmBackend::Anthropic && api_key.is_empty() {
            return Err(
                "ANTHROPIC_API_KEY not set. Create a .env file with your API key.".to_string(),
            );
        }

        // OpenAI API key — required when backend is openai
        let openai_api_key = std::env::var("OPENAI_API_KEY").ok();
        if backend == LlmBackend::OpenAi && openai_api_key.is_none() {
            return Err(
                "OPENAI_API_KEY not set. Set it in your .env file or environment.".to_string(),
            );
        }

        // Gemini API key — required when backend is gemini
        let gemini_api_key = std::env::var("GEMINI_API_KEY").ok();
        if backend == LlmBackend::Gemini && gemini_api_key.is_none() {
            return Err(
                "GEMINI_API_KEY not set. Set it in your .env file or environment.".to_string(),
            );
        }

        let model = std::env::var("CLAWED_MODEL")
            .unwrap_or_else(|_| "claude-sonnet-4-20250514".to_string());

        let openai_model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o".to_string());

        let gemini_model =
            std::env::var("GEMINI_MODEL").unwrap_or_else(|_| "gemini-2.5-flash".to_string());

        let claude_cli_model =
            std::env::var("CLAUDE_CLI_MODEL").unwrap_or_else(|_| "opus4.6".to_string());

        let claude_cli_timeout_secs = std::env::var("CLAUDE_CLI_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(300);

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
            openai_api_key,
            openai_model,
            gemini_api_key,
            gemini_model,
            claude_cli_model,
            claude_cli_timeout_secs,
            skills_dir,
            max_turns,
            skills_enabled: true,
        })
    }
}
