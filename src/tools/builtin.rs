//! Built-in utility tools: echo, time, json.
//!
//! These are safe, read-only tools with no side effects. They appear in the
//! `READ_ONLY_TOOLS` list in `skills/attenuation.rs` and are available even
//! when an `Installed` (untrusted) skill is active.

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::error::ToolError;
use crate::tools::{Tool, ToolContext, ToolOutput, require_param, require_str};

// ---------------------------------------------------------------------------
// EchoTool
// ---------------------------------------------------------------------------

/// Simple echo tool for testing tool execution.
pub struct EchoTool;

impl EchoTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }

    fn description(&self) -> &str {
        "Echoes back the input message. Useful for testing tool execution."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "The message to echo back"
                }
            },
            "required": ["message"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let message = require_str(&params, "message")?;
        Ok(ToolOutput::text(message))
    }
}

// ---------------------------------------------------------------------------
// TimeTool
// ---------------------------------------------------------------------------

/// Tool for getting current time and performing date operations.
pub struct TimeTool;

impl TimeTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for TimeTool {
    fn name(&self) -> &str {
        "time"
    }

    fn description(&self) -> &str {
        "Get current time, convert timezones, or calculate time differences."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["now", "parse", "diff"],
                    "description": "The time operation to perform"
                },
                "timestamp": {
                    "type": "string",
                    "description": "ISO 8601 timestamp (for parse/diff operations)"
                },
                "timestamp2": {
                    "type": "string",
                    "description": "Second timestamp (for diff operation)"
                }
            },
            "required": ["operation"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let operation = require_str(&params, "operation")?;

        let result = match operation {
            "now" => {
                let now = Utc::now();
                serde_json::json!({
                    "iso": now.to_rfc3339(),
                    "unix": now.timestamp(),
                    "unix_millis": now.timestamp_millis()
                })
            }
            "parse" => {
                let timestamp = require_str(&params, "timestamp")?;
                let dt: DateTime<Utc> = timestamp.parse().map_err(|e| {
                    ToolError::InvalidParameters(format!("invalid timestamp: {}", e))
                })?;
                serde_json::json!({
                    "iso": dt.to_rfc3339(),
                    "unix": dt.timestamp(),
                    "unix_millis": dt.timestamp_millis()
                })
            }
            "diff" => {
                let ts1 = require_str(&params, "timestamp")?;
                let ts2 = require_str(&params, "timestamp2")?;
                let dt1: DateTime<Utc> = ts1.parse().map_err(|e| {
                    ToolError::InvalidParameters(format!("invalid timestamp: {}", e))
                })?;
                let dt2: DateTime<Utc> = ts2.parse().map_err(|e| {
                    ToolError::InvalidParameters(format!("invalid timestamp2: {}", e))
                })?;
                let diff = dt2.signed_duration_since(dt1);
                serde_json::json!({
                    "seconds": diff.num_seconds(),
                    "minutes": diff.num_minutes(),
                    "hours": diff.num_hours(),
                    "days": diff.num_days()
                })
            }
            _ => {
                return Err(ToolError::InvalidParameters(format!(
                    "unknown operation: {}",
                    operation
                )));
            }
        };

        Ok(ToolOutput::text(
            serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()),
        ))
    }
}

// ---------------------------------------------------------------------------
// JsonTool
// ---------------------------------------------------------------------------

/// Tool for JSON manipulation (parse, query, stringify, validate).
pub struct JsonTool;

impl JsonTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for JsonTool {
    fn name(&self) -> &str {
        "json"
    }

    fn description(&self) -> &str {
        "Parse, query, and transform JSON data. Supports JSONPath-like queries."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["parse", "query", "stringify", "validate"],
                    "description": "The JSON operation to perform"
                },
                "data": {
                    "description": "JSON input data. Pass a string for parse, or any JSON value otherwise."
                },
                "path": {
                    "type": "string",
                    "description": "JSONPath-like path for query operation (e.g., 'foo.bar[0].baz')"
                }
            },
            "required": ["operation", "data"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {
        let operation = require_str(&params, "operation")?;
        let data = require_param(&params, "data")?;

        let result = match operation {
            "parse" => {
                let json_str = data.as_str().ok_or_else(|| {
                    ToolError::InvalidParameters(
                        "'data' must be a string for parse operation".to_string(),
                    )
                })?;
                let parsed: serde_json::Value = serde_json::from_str(json_str)
                    .map_err(|e| ToolError::InvalidParameters(format!("invalid JSON: {}", e)))?;
                parsed
            }
            "stringify" => {
                let value = parse_json_input(data)?;
                let json_str = serde_json::to_string_pretty(&value).map_err(|e| {
                    ToolError::ExecutionFailed(format!("failed to stringify: {}", e))
                })?;
                serde_json::Value::String(json_str)
            }
            "query" => {
                let path = params.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
                    ToolError::InvalidParameters("missing 'path' parameter for query".to_string())
                })?;
                let value = parse_json_input(data)?;
                query_json(&value, path)?
            }
            "validate" => {
                let is_valid = data
                    .as_str()
                    .map(|s| serde_json::from_str::<serde_json::Value>(s).is_ok())
                    .unwrap_or(false);
                serde_json::json!({ "valid": is_valid })
            }
            _ => {
                return Err(ToolError::InvalidParameters(format!(
                    "unknown operation: {}",
                    operation
                )));
            }
        };

        Ok(ToolOutput::text(
            serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()),
        ))
    }
}

fn parse_json_input(data: &serde_json::Value) -> Result<serde_json::Value, ToolError> {
    let json_str = data
        .as_str()
        .ok_or_else(|| ToolError::InvalidParameters("'data' must be a JSON string".to_string()))?;
    serde_json::from_str(json_str)
        .map_err(|e| ToolError::InvalidParameters(format!("invalid JSON input: {}", e)))
}

/// Simple JSONPath-like query implementation.
fn query_json(data: &serde_json::Value, path: &str) -> Result<serde_json::Value, ToolError> {
    let mut current = data;

    for segment in path.split('.') {
        if segment.is_empty() {
            continue;
        }

        if let Some((field, index_str)) = segment.split_once('[') {
            if !field.is_empty() {
                current = current.get(field).ok_or_else(|| {
                    ToolError::ExecutionFailed(format!("field not found: {}", field))
                })?;
            }

            let index_str = index_str.trim_end_matches(']');
            let index: usize = index_str.parse().map_err(|_| {
                ToolError::InvalidParameters(format!("invalid array index: {}", index_str))
            })?;

            current = current.get(index).ok_or_else(|| {
                ToolError::ExecutionFailed(format!("array index out of bounds: {}", index))
            })?;
        } else {
            current = current.get(segment).ok_or_else(|| {
                ToolError::ExecutionFailed(format!("field not found: {}", segment))
            })?;
        }
    }

    Ok(current.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_query_json() {
        let data = serde_json::json!({
            "foo": {
                "bar": [1, 2, 3],
                "baz": "hello"
            }
        });

        assert_eq!(
            query_json(&data, "foo.baz").unwrap(),
            serde_json::json!("hello")
        );
        assert_eq!(
            query_json(&data, "foo.bar[0]").unwrap(),
            serde_json::json!(1)
        );
        assert_eq!(
            query_json(&data, "foo.bar[2]").unwrap(),
            serde_json::json!(3)
        );
    }

    #[test]
    fn test_parse_json_input_accepts_valid_json_string() {
        let input = serde_json::json!("{\"ok\":true}");
        let parsed = parse_json_input(&input).unwrap();
        assert_eq!(parsed, serde_json::json!({"ok": true}));
    }

    #[test]
    fn test_parse_json_input_rejects_invalid_json_string() {
        let input = serde_json::json!("{not valid json}");
        let err = parse_json_input(&input).unwrap_err();
        assert!(err.to_string().contains("invalid JSON input"));
    }

    #[test]
    fn test_json_tool_schema_data_is_freeform() {
        let schema = JsonTool.parameters_schema();
        let data = schema
            .get("properties")
            .and_then(|p| p.get("data"))
            .expect("data schema missing");
        assert!(
            data.get("type").is_none(),
            "data schema should not have a 'type' to stay freeform"
        );
    }

    #[tokio::test]
    async fn test_echo_tool() {
        let tool = EchoTool::new();
        let ctx = ToolContext::default();
        let result = tool
            .execute(serde_json::json!({"message": "hello world"}), &ctx)
            .await
            .unwrap();
        assert_eq!(result.content, "hello world");
    }

    #[tokio::test]
    async fn test_time_now() {
        let tool = TimeTool::new();
        let ctx = ToolContext::default();
        let result = tool
            .execute(serde_json::json!({"operation": "now"}), &ctx)
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        assert!(parsed.get("iso").is_some());
        assert!(parsed.get("unix").is_some());
        assert!(parsed.get("unix_millis").is_some());
    }

    #[tokio::test]
    async fn test_time_parse() {
        let tool = TimeTool::new();
        let ctx = ToolContext::default();
        let result = tool
            .execute(
                serde_json::json!({"operation": "parse", "timestamp": "2024-01-15T10:30:00Z"}),
                &ctx,
            )
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(parsed["unix"], 1705314600);
    }

    #[tokio::test]
    async fn test_time_diff() {
        let tool = TimeTool::new();
        let ctx = ToolContext::default();
        let result = tool
            .execute(
                serde_json::json!({
                    "operation": "diff",
                    "timestamp": "2024-01-15T00:00:00Z",
                    "timestamp2": "2024-01-16T00:00:00Z"
                }),
                &ctx,
            )
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(parsed["days"], 1);
        assert_eq!(parsed["hours"], 24);
        assert_eq!(parsed["seconds"], 86400);
    }

    #[tokio::test]
    async fn test_json_parse() {
        let tool = JsonTool::new();
        let ctx = ToolContext::default();
        let result = tool
            .execute(
                serde_json::json!({"operation": "parse", "data": "{\"key\": \"value\"}"}),
                &ctx,
            )
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(parsed["key"], "value");
    }

    #[tokio::test]
    async fn test_json_validate() {
        let tool = JsonTool::new();
        let ctx = ToolContext::default();

        let valid = tool
            .execute(
                serde_json::json!({"operation": "validate", "data": "{\"ok\": true}"}),
                &ctx,
            )
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&valid.content).unwrap();
        assert_eq!(parsed["valid"], true);

        let invalid = tool
            .execute(
                serde_json::json!({"operation": "validate", "data": "{not valid}"}),
                &ctx,
            )
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&invalid.content).unwrap();
        assert_eq!(parsed["valid"], false);
    }

    #[tokio::test]
    async fn test_json_query() {
        let tool = JsonTool::new();
        let ctx = ToolContext::default();
        let result = tool
            .execute(
                serde_json::json!({
                    "operation": "query",
                    "data": "{\"users\": [{\"name\": \"alice\"}, {\"name\": \"bob\"}]}",
                    "path": "users[1].name"
                }),
                &ctx,
            )
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(parsed, "bob");
    }

    #[tokio::test]
    async fn test_json_stringify() {
        let tool = JsonTool::new();
        let ctx = ToolContext::default();
        let result = tool
            .execute(
                serde_json::json!({"operation": "stringify", "data": "{\"key\": 42}"}),
                &ctx,
            )
            .await
            .unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        // stringify returns a JSON string of the pretty-printed value
        assert!(parsed.is_string());
        assert!(parsed.as_str().unwrap().contains("42"));
    }
}
