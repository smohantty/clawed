//! Skill discovery and on-demand loading tools.
//!
//! `SkillListTool` — returns the full skill catalog as JSON.
//! `LoadSkillTool` — loads a skill's SKILL.md body or a reference file on demand.

use std::path::{Component, Path};
use std::sync::Arc;

use async_trait::async_trait;

use crate::error::ToolError;
use crate::skills::{LoadedSkill, SkillSource, escape_skill_content, escape_xml_attr};
use crate::tools::{Tool, ToolContext, ToolOutput, require_str};

/// Maximum size for a loaded reference file (32 KiB).
const MAX_RESOURCE_SIZE: u64 = 32 * 1024;

// ---------------------------------------------------------------------------
// SkillListTool
// ---------------------------------------------------------------------------

/// Returns a JSON array of all available skills with metadata.
pub struct SkillListTool {
    skills: Arc<Vec<LoadedSkill>>,
}

impl SkillListTool {
    pub fn new(skills: Arc<Vec<LoadedSkill>>) -> Self {
        Self { skills }
    }
}

#[async_trait]
impl Tool for SkillListTool {
    fn name(&self) -> &str {
        "skill_list"
    }

    fn description(&self) -> &str {
        "List all available skills with their name, description, and trust level."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "verbose": {
                    "type": "boolean",
                    "description": "If true, include activation keywords and tags (default: false)"
                }
            }
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let verbose = params
            .get("verbose")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        tracing::event!(
            target: "clawed::audit",
            tracing::Level::DEBUG,
            tool = "skill_list",
            verbose,
            known_skill_count = self.skills.len(),
            "Executing skill_list tool"
        );

        let entries: Vec<serde_json::Value> = self
            .skills
            .iter()
            .map(|s| {
                let mut entry = serde_json::json!({
                    "name": s.manifest.name,
                    "description": s.manifest.description,
                    "trust": s.trust.to_string(),
                });
                if verbose {
                    entry["keywords"] = serde_json::json!(s.manifest.activation.keywords);
                    entry["tags"] = serde_json::json!(s.manifest.activation.tags);
                }
                entry
            })
            .collect();

        let json = serde_json::to_string_pretty(&entries).unwrap_or_else(|_| "[]".to_string());
        tracing::event!(
            target: "clawed::audit",
            tracing::Level::DEBUG,
            tool = "skill_list",
            output_len = json.len(),
            output_preview = %crate::logging::preview_text(&json, 1200),
            "Completed skill_list tool"
        );
        Ok(ToolOutput::text(json))
    }
}

// ---------------------------------------------------------------------------
// LoadSkillTool
// ---------------------------------------------------------------------------

/// Loads a skill's full instruction content or a reference file on demand.
pub struct LoadSkillTool {
    skills: Arc<Vec<LoadedSkill>>,
}

impl LoadSkillTool {
    pub fn new(skills: Arc<Vec<LoadedSkill>>) -> Self {
        Self { skills }
    }
}

#[async_trait]
impl Tool for LoadSkillTool {
    fn name(&self) -> &str {
        "load_skill"
    }

    fn description(&self) -> &str {
        "Load a skill's instructions or a reference file from a skill directory. \
         Call with just a name to get the full SKILL.md body. Add a path \
         (e.g. \"references/API.md\") to load a specific resource file."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "The skill name to load"
                },
                "path": {
                    "type": "string",
                    "description": "Optional relative path to a resource file within the skill directory (e.g. \"references/API.md\")"
                }
            },
            "required": ["name"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let name = require_str(&params, "name")?;
        let resource_path = params.get("path").and_then(|v| v.as_str());
        tracing::event!(
            target: "clawed::audit",
            tracing::Level::DEBUG,
            tool = "load_skill",
            skill_name = %name,
            resource_path = ?resource_path,
            "Executing load_skill tool"
        );

        // Find the skill by name
        let skill = self
            .skills
            .iter()
            .find(|s| s.manifest.name == name)
            .ok_or_else(|| ToolError::InvalidParameters(format!("skill '{}' not found", name)))?;

        let trust = skill.trust.to_string();
        let escaped_name = escape_xml_attr(name);
        let dir = escape_xml_attr(&skill.source_dir().display().to_string());

        match resource_path {
            None => {
                // Return the full SKILL.md body
                let content = escape_skill_content(&skill.prompt_content);
                let output = format!(
                    "<skill name=\"{}\" trust=\"{}\" dir=\"{}\">\n{}\n</skill>",
                    escaped_name, trust, dir, content
                );
                tracing::event!(
                    target: "clawed::audit",
                    tracing::Level::DEBUG,
                    tool = "load_skill",
                    skill_name = %name,
                    mode = "full-skill",
                    output_len = output.len(),
                    output_preview = %crate::logging::preview_text(&output, 1200),
                    "Completed load_skill tool"
                );
                Ok(ToolOutput::text(output))
            }
            Some(rel_path) => {
                // Load a specific resource file from the skill directory
                let skill_dir = match &skill.source {
                    SkillSource::User(p) => p.clone(),
                };

                // Validate the relative path for safety
                validate_resource_path(rel_path)?;

                let full_path = skill_dir.join(rel_path);

                // Verify the resolved path is under the skill directory
                let canonical_dir = tokio::fs::canonicalize(&skill_dir).await.map_err(|e| {
                    ToolError::ExecutionFailed(format!("cannot resolve skill directory: {}", e))
                })?;
                let canonical_file = tokio::fs::canonicalize(&full_path).await.map_err(|e| {
                    ToolError::ExecutionFailed(format!(
                        "cannot resolve resource path '{}': {}",
                        rel_path, e
                    ))
                })?;

                if !canonical_file.starts_with(&canonical_dir) {
                    return Err(ToolError::InvalidParameters(
                        "path traversal detected: resource must be within the skill directory"
                            .to_string(),
                    ));
                }

                // Check file size
                let metadata = tokio::fs::metadata(&canonical_file).await.map_err(|e| {
                    ToolError::ExecutionFailed(format!(
                        "cannot read resource '{}': {}",
                        rel_path, e
                    ))
                })?;

                if metadata.len() > MAX_RESOURCE_SIZE {
                    return Err(ToolError::ExecutionFailed(format!(
                        "resource file too large: {} bytes (max {} bytes)",
                        metadata.len(),
                        MAX_RESOURCE_SIZE
                    )));
                }

                let content = tokio::fs::read_to_string(&canonical_file)
                    .await
                    .map_err(|e| {
                        ToolError::ExecutionFailed(format!(
                            "failed to read resource '{}': {}",
                            rel_path, e
                        ))
                    })?;

                let output = format!(
                    "<skill-resource name=\"{}\" path=\"{}\" trust=\"{}\">\n{}\n</skill-resource>",
                    escaped_name,
                    escape_xml_attr(rel_path),
                    trust,
                    content
                );
                tracing::event!(
                    target: "clawed::audit",
                    tracing::Level::DEBUG,
                    tool = "load_skill",
                    skill_name = %name,
                    mode = "resource",
                    resource_path = %rel_path,
                    output_len = output.len(),
                    output_preview = %crate::logging::preview_text(&output, 1200),
                    "Completed load_skill resource read"
                );
                Ok(ToolOutput::text(output))
            }
        }
    }
}

/// Validate that a resource path is safe (no traversal, no absolute paths, no symlink tricks).
fn validate_resource_path(path: &str) -> Result<(), ToolError> {
    if path.is_empty() {
        return Err(ToolError::InvalidParameters(
            "resource path cannot be empty".to_string(),
        ));
    }

    let p = Path::new(path);

    // Reject absolute paths
    if p.is_absolute() {
        return Err(ToolError::InvalidParameters(
            "resource path must be relative".to_string(),
        ));
    }

    // Reject components that escape the directory
    for component in p.components() {
        match component {
            Component::ParentDir => {
                return Err(ToolError::InvalidParameters(
                    "resource path must not contain '..'".to_string(),
                ));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(ToolError::InvalidParameters(
                    "resource path must be relative".to_string(),
                ));
            }
            _ => {}
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_resource_path_accepts_valid() {
        assert!(validate_resource_path("references/API.md").is_ok());
        assert!(validate_resource_path("scripts/setup.sh").is_ok());
        assert!(validate_resource_path("file.txt").is_ok());
    }

    #[test]
    fn test_validate_resource_path_rejects_traversal() {
        assert!(validate_resource_path("../etc/passwd").is_err());
        assert!(validate_resource_path("references/../../secret").is_err());
    }

    #[test]
    fn test_validate_resource_path_rejects_absolute() {
        assert!(validate_resource_path("/etc/passwd").is_err());
    }

    #[test]
    fn test_validate_resource_path_rejects_empty() {
        assert!(validate_resource_path("").is_err());
    }
}
