//! File operation tools for reading, writing, listing, and patching files.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use tokio::fs;

use crate::error::ToolError;
use crate::tools::{Tool, ToolContext, ToolOutput, require_str};

const MAX_READ_SIZE: u64 = 1024 * 1024;
const MAX_WRITE_SIZE: usize = 5 * 1024 * 1024;
const MAX_DIR_ENTRIES: usize = 500;

/// Normalize a path by resolving `.` and `..` components lexically.
fn normalize_lexical(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                if components
                    .last()
                    .is_some_and(|c| matches!(c, std::path::Component::Normal(_)))
                {
                    components.pop();
                }
            }
            std::path::Component::CurDir => {}
            other => components.push(other),
        }
    }
    components.iter().collect()
}

/// Resolve a path to absolute, using working_dir as base for relative paths.
fn resolve_path(path_str: &str, working_dir: &Path) -> PathBuf {
    let path = PathBuf::from(path_str);
    if path.is_absolute() {
        path.canonicalize()
            .unwrap_or_else(|_| normalize_lexical(&path))
    } else {
        let joined = working_dir.join(&path);
        joined
            .canonicalize()
            .unwrap_or_else(|_| normalize_lexical(&joined))
    }
}

// --- ReadFileTool ---

pub struct ReadFileTool;

impl ReadFileTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read a file from the filesystem. Returns file content as text. \
         For large files, specify offset and limit to read a portion."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to read"
                },
                "offset": {
                    "type": "integer",
                    "description": "Line number to start reading from (1-indexed, optional)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to read (optional)"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {

        let path_str = require_str(&params, "path")?;
        let resolved = resolve_path(path_str, &ctx.working_dir);

        // Check file size
        let metadata = fs::metadata(&resolved).await.map_err(|e| {
            ToolError::ExecutionFailed(format!("Cannot read '{}': {}", path_str, e))
        })?;

        if metadata.len() > MAX_READ_SIZE {
            return Err(ToolError::ExecutionFailed(format!(
                "File too large: {} bytes (max {} bytes)",
                metadata.len(),
                MAX_READ_SIZE
            )));
        }

        let content = fs::read_to_string(&resolved).await.map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to read '{}': {}", path_str, e))
        })?;

        // Handle offset/limit
        let offset = params
            .get("offset")
            .and_then(|v| v.as_u64())
            .map(|v| v.saturating_sub(1) as usize) // Convert to 0-indexed
            .unwrap_or(0);
        let limit = params
            .get("limit")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);

        let lines: Vec<&str> = content.lines().collect();
        let selected: Vec<String> = lines
            .iter()
            .enumerate()
            .skip(offset)
            .take(limit.unwrap_or(lines.len()))
            .map(|(i, line)| format!("{:>4}\t{}", i + 1, line))
            .collect();

        let result = if selected.is_empty() {
            format!("(empty file: {})", path_str)
        } else {
            selected.join("\n")
        };

        Ok(ToolOutput::text(result))
    }
}

// --- WriteFileTool ---

pub struct WriteFileTool;

impl WriteFileTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write content to a file. Creates the file if it doesn't exist, \
         overwrites if it does. Creates parent directories as needed."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {

        let path_str = require_str(&params, "path")?;
        let content = require_str(&params, "content")?;

        if content.len() > MAX_WRITE_SIZE {
            return Err(ToolError::ExecutionFailed(format!(
                "Content too large: {} bytes (max {} bytes)",
                content.len(),
                MAX_WRITE_SIZE
            )));
        }

        let resolved = resolve_path(path_str, &ctx.working_dir);

        // Create parent directories
        if let Some(parent) = resolved.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to create directories: {}", e))
            })?;
        }

        fs::write(&resolved, content).await.map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to write '{}': {}", path_str, e))
        })?;

        Ok(ToolOutput::text(
            format!("Wrote {} bytes to {}", content.len(), path_str),
        ))
    }
}

// --- ListDirTool ---

pub struct ListDirTool;

impl ListDirTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ListDirTool {
    fn name(&self) -> &str {
        "list_dir"
    }

    fn description(&self) -> &str {
        "List contents of a directory. Returns file names, sizes, and types."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the directory to list (defaults to working directory)"
                }
            },
            "required": []
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {

        let path_str = params
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or(".");
        let resolved = resolve_path(path_str, &ctx.working_dir);

        let mut entries = fs::read_dir(&resolved).await.map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to read directory '{}': {}", path_str, e))
        })?;

        let mut items = Vec::new();
        let mut count = 0;

        while let Ok(Some(entry)) = entries.next_entry().await {
            if count >= MAX_DIR_ENTRIES {
                items.push(format!("... and more (truncated at {})", MAX_DIR_ENTRIES));
                break;
            }

            let file_name = entry.file_name().to_string_lossy().to_string();
            let file_type = if let Ok(ft) = entry.file_type().await {
                if ft.is_dir() {
                    "dir"
                } else if ft.is_symlink() {
                    "link"
                } else {
                    "file"
                }
            } else {
                "?"
            };

            let size = if file_type == "file" {
                entry
                    .metadata()
                    .await
                    .map(|m| format_size(m.len()))
                    .unwrap_or_else(|_| "?".to_string())
            } else {
                "-".to_string()
            };

            items.push(format!("{:<6} {:<10} {}", file_type, size, file_name));
            count += 1;
        }

        items.sort();

        let result = if items.is_empty() {
            format!("(empty directory: {})", path_str)
        } else {
            items.join("\n")
        };

        Ok(ToolOutput::text(result))
    }
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{}B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1}GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

// --- ApplyPatchTool ---

pub struct ApplyPatchTool;

impl ApplyPatchTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ApplyPatchTool {
    fn name(&self) -> &str {
        "apply_patch"
    }

    fn description(&self) -> &str {
        "Apply a search-and-replace patch to a file. Finds the exact 'search' string \
         and replaces it with 'replace'. Use for precise file edits."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to patch"
                },
                "search": {
                    "type": "string",
                    "description": "Exact string to find in the file"
                },
                "replace": {
                    "type": "string",
                    "description": "String to replace it with"
                }
            },
            "required": ["path", "search", "replace"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {

        let path_str = require_str(&params, "path")?;
        let search = require_str(&params, "search")?;
        let replace = require_str(&params, "replace")?;

        let resolved = resolve_path(path_str, &ctx.working_dir);

        let content = fs::read_to_string(&resolved).await.map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to read '{}': {}", path_str, e))
        })?;

        let count = content.matches(search).count();
        if count == 0 {
            return Err(ToolError::ExecutionFailed(format!(
                "Search string not found in '{}'",
                path_str
            )));
        }

        if count > 1 {
            return Err(ToolError::ExecutionFailed(format!(
                "Search string found {} times in '{}' (must be unique). Provide more context.",
                count, path_str
            )));
        }

        let new_content = content.replacen(search, replace, 1);

        fs::write(&resolved, &new_content).await.map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to write '{}': {}", path_str, e))
        })?;

        Ok(ToolOutput::text(
            format!("Patched {} (1 replacement)", path_str),
        ))
    }
}
