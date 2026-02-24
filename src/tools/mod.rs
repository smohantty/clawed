//! Tool trait, registry, and types.

pub mod file;
pub mod shell;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::error::ToolError;
use crate::llm::ToolDefinition;

/// Output from a tool execution.
#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub content: String,
}

impl ToolOutput {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: text.into(),
        }
    }
}

/// Minimal context passed to tools.
pub struct ToolContext {
    pub working_dir: PathBuf,
    pub extra_env: HashMap<String, String>,
}

impl Default for ToolContext {
    fn default() -> Self {
        Self {
            working_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            extra_env: HashMap::new(),
        }
    }
}

/// Trait for tools that the agent can use.
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError>;
}

/// Extract a required string parameter from a JSON object.
pub fn require_str<'a>(params: &'a serde_json::Value, name: &str) -> Result<&'a str, ToolError> {
    params
        .get(name)
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::InvalidParameters(format!("missing '{}' parameter", name)))
}

/// Registry of available tools.
pub struct ToolRegistry {
    tools: RwLock<HashMap<String, Arc<dyn Tool>>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
        }
    }

    pub async fn register(&self, tool: Arc<dyn Tool>) {
        let name = tool.name().to_string();
        self.tools.write().await.insert(name, tool);
    }

    pub async fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.read().await.get(name).cloned()
    }

    pub async fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .read()
            .await
            .values()
            .map(|tool| ToolDefinition {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                parameters: tool.parameters_schema(),
            })
            .collect()
    }

    /// Execute a tool by name with the given arguments.
    pub async fn execute(
        &self,
        name: &str,
        arguments: &serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let tool = self
            .get(name)
            .await
            .ok_or_else(|| ToolError::NotFound(format!("Tool '{}' not found", name)))?;
        tool.execute(arguments.clone(), ctx).await
    }

    /// Register all dev tools (shell, file operations).
    pub async fn register_dev_tools(&self) {
        self.register(Arc::new(shell::ShellTool::new())).await;
        self.register(Arc::new(file::ReadFileTool::new())).await;
        self.register(Arc::new(file::WriteFileTool::new())).await;
        self.register(Arc::new(file::ListDirTool::new())).await;
        self.register(Arc::new(file::ApplyPatchTool::new())).await;
        tracing::info!("Registered 5 development tools");
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
