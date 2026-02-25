//! Tool trait, registry, and types.

pub mod builtin;
pub mod file;
pub mod shell;
pub mod skill_tools;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::error::ToolError;
use crate::llm::ToolDefinition;
use crate::skills::LoadedSkill;

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

/// Extract a required parameter of any type from a JSON object.
pub fn require_param<'a>(
    params: &'a serde_json::Value,
    name: &str,
) -> Result<&'a serde_json::Value, ToolError> {
    params
        .get(name)
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

    /// Register all dev tools (shell, file operations, builtins).
    pub async fn register_dev_tools(&self) {
        let shell_tool = Arc::new(shell::ShellTool::new());
        let read_file_tool = Arc::new(file::ReadFileTool::new());
        let write_file_tool = Arc::new(file::WriteFileTool::new());
        let list_dir_tool = Arc::new(file::ListDirTool::new());
        let apply_patch_tool = Arc::new(file::ApplyPatchTool::new());
        let echo_tool = Arc::new(builtin::EchoTool::new());
        let time_tool = Arc::new(builtin::TimeTool::new());
        let json_tool = Arc::new(builtin::JsonTool::new());

        let development_tool_names = [
            shell_tool.name().to_string(),
            read_file_tool.name().to_string(),
            write_file_tool.name().to_string(),
            list_dir_tool.name().to_string(),
            apply_patch_tool.name().to_string(),
            echo_tool.name().to_string(),
            time_tool.name().to_string(),
            json_tool.name().to_string(),
        ];
        let builtin_tool_names = [
            echo_tool.name().to_string(),
            time_tool.name().to_string(),
            json_tool.name().to_string(),
        ];

        self.register(shell_tool).await;
        self.register(read_file_tool).await;
        self.register(write_file_tool).await;
        self.register(list_dir_tool).await;
        self.register(apply_patch_tool).await;
        self.register(echo_tool).await;
        self.register(time_tool).await;
        self.register(json_tool).await;

        tracing::info!(
            count = builtin_tool_names.len(),
            builtin_tools = ?builtin_tool_names,
            "Registered builtin tools"
        );
        tracing::info!(
            count = development_tool_names.len(),
            development_tools = ?development_tool_names,
            "Registered development tools"
        );
    }

    /// Register skill discovery and loading tools.
    pub async fn register_skill_tools(&self, skills: Arc<Vec<LoadedSkill>>) {
        let skill_list_tool = Arc::new(skill_tools::SkillListTool::new(skills.clone()));
        let load_skill_tool = Arc::new(skill_tools::LoadSkillTool::new(skills));
        let skill_tool_names = [
            skill_list_tool.name().to_string(),
            load_skill_tool.name().to_string(),
        ];

        self.register(skill_list_tool).await;
        self.register(load_skill_tool).await;

        tracing::info!(
            count = skill_tool_names.len(),
            skill_tools = ?skill_tool_names,
            "Registered skill tools"
        );
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
