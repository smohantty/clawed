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
