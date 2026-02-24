//! LLM integration for the agent.

pub mod provider;
pub mod reasoning;
pub mod rig_adapter;

pub use provider::{
    ChatMessage, CompletionRequest, LlmProvider, ToolCall,
    ToolCompletionRequest, ToolDefinition,
};
pub use reasoning::{Reasoning, ReasoningContext, RespondResult};
pub use rig_adapter::RigAdapter;

use std::sync::Arc;

use rig::client::CompletionClient;

use crate::config::{ClawedConfig, LlmBackend};
use crate::error::LlmError;

/// Create an LLM provider based on configuration.
pub fn create_llm_provider(config: &ClawedConfig) -> Result<Arc<dyn LlmProvider>, LlmError> {
    match config.backend {
        LlmBackend::Anthropic => create_anthropic_provider(config),
        LlmBackend::OpenAi => create_openai_provider(config),
        LlmBackend::Gemini => create_gemini_provider(config),
    }
}

fn create_anthropic_provider(config: &ClawedConfig) -> Result<Arc<dyn LlmProvider>, LlmError> {
    use rig::providers::anthropic;

    let client: anthropic::Client =
        anthropic::Client::new(&config.api_key).map_err(|e| LlmError::RequestFailed {
            provider: "anthropic".to_string(),
            reason: format!("Failed to create Anthropic client: {}", e),
        })?;

    let model = client.completion_model(&config.model);
    tracing::info!("Using Anthropic direct API (model: {})", config.model);
    Ok(Arc::new(RigAdapter::new(model, &config.model)))
}

fn create_openai_provider(config: &ClawedConfig) -> Result<Arc<dyn LlmProvider>, LlmError> {
    use rig::providers::openai;

    let api_key = config.openai_api_key.as_deref().ok_or_else(|| LlmError::RequestFailed {
        provider: "openai".to_string(),
        reason: "OPENAI_API_KEY not set".to_string(),
    })?;

    // Use CompletionsClient (Chat Completions API) instead of the default Client
    // (Responses API). The Responses API has call_id threading issues with rig-core.
    let client: openai::CompletionsClient =
        openai::Client::new(api_key)
            .map_err(|e| LlmError::RequestFailed {
                provider: "openai".to_string(),
                reason: format!("Failed to create OpenAI client: {}", e),
            })?
            .completions_api();

    let model = client.completion_model(&config.openai_model);
    tracing::info!("Using OpenAI direct API (model: {})", config.openai_model);
    Ok(Arc::new(RigAdapter::new(model, &config.openai_model)))
}

fn create_gemini_provider(config: &ClawedConfig) -> Result<Arc<dyn LlmProvider>, LlmError> {
    use rig::providers::gemini;

    let api_key = config.gemini_api_key.as_deref().ok_or_else(|| LlmError::RequestFailed {
        provider: "gemini".to_string(),
        reason: "GEMINI_API_KEY not set".to_string(),
    })?;

    let client = gemini::Client::new(api_key).map_err(|e| LlmError::RequestFailed {
        provider: "gemini".to_string(),
        reason: format!("Failed to create Gemini client: {}", e),
    })?;
    let model = client.completion_model(&config.gemini_model);
    tracing::info!("Using Gemini direct API (model: {})", config.gemini_model);
    Ok(Arc::new(RigAdapter::new(model, &config.gemini_model)))
}
