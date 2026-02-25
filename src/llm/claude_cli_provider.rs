//! Claude CLI-backed provider that runs `claude -p` for one-shot completions.

use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::error::LlmError;
use crate::llm::provider::{
    ChatMessage, CompletionRequest, CompletionResponse, LlmProvider, Role, ToolCompletionRequest,
    ToolCompletionResponse, ToolDefinition,
};

const PROVIDER_NAME: &str = "claude_cli";
const ERROR_SNIPPET_LIMIT: usize = 2000;

#[derive(Debug, Clone)]
pub struct ClaudeCliProvider {
    model: String,
    timeout: Duration,
}

#[derive(Debug)]
struct ParsedOutput {
    content: String,
    input_tokens: u32,
    output_tokens: u32,
}

impl ClaudeCliProvider {
    pub fn new(model: String, timeout_secs: u64) -> Result<Self, LlmError> {
        let requested_model = model.trim().to_string();
        if requested_model.is_empty() {
            return Err(LlmError::RequestFailed {
                provider: PROVIDER_NAME.to_string(),
                reason: "CLAUDE_CLI_MODEL cannot be empty".to_string(),
            });
        }
        let model = normalize_model_alias(&requested_model);
        if model != requested_model {
            tracing::debug!(
                requested = %requested_model,
                resolved = %model,
                "Resolved Claude CLI model alias"
            );
        }

        Ok(Self {
            model,
            timeout: Duration::from_secs(timeout_secs.max(1)),
        })
    }

    async fn run_completion(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
    ) -> Result<ParsedOutput, LlmError> {
        let prompt = build_prompt(messages, tools);
        let command_preview = format!(
            "claude -p --input-format text --output-format json --tools \"\" --no-session-persistence --model {}",
            shell_quote(&self.model)
        );
        tracing::debug!(
            command = %command_preview,
            stdin_prompt_len = prompt.len(),
            stdin_prompt_preview = %crate::logging::preview_text(&prompt, 1200),
            "Invoking Claude CLI command"
        );

        let mut cmd = Command::new("claude");
        cmd.arg("-p")
            .arg("--input-format")
            .arg("text")
            .arg("--output-format")
            .arg("json")
            .arg("--tools")
            .arg("")
            .arg("--no-session-persistence")
            .arg("--model")
            .arg(&self.model);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd.kill_on_drop(true);

        let mut child = cmd.spawn().map_err(|e| {
            let reason = if e.kind() == std::io::ErrorKind::NotFound {
                "claude command not found in PATH".to_string()
            } else {
                format!("failed to execute claude command: {}", e)
            };
            LlmError::RequestFailed {
                provider: PROVIDER_NAME.to_string(),
                reason,
            }
        })?;

        let output = tokio::time::timeout(self.timeout, async {
            if let Some(mut stdin) = child.stdin.take() {
                stdin
                    .write_all(prompt.as_bytes())
                    .await
                    .map_err(|e| LlmError::RequestFailed {
                        provider: PROVIDER_NAME.to_string(),
                        reason: format!("failed writing prompt to claude stdin: {}", e),
                    })?;
            }
            child
                .wait_with_output()
                .await
                .map_err(|e| LlmError::RequestFailed {
                    provider: PROVIDER_NAME.to_string(),
                    reason: format!("failed waiting for claude command: {}", e),
                })
        })
        .await
        .map_err(|_| LlmError::RequestFailed {
            provider: PROVIDER_NAME.to_string(),
            reason: format!("claude command timed out after {}s", self.timeout.as_secs()),
        })??;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        tracing::event!(
            target: "clawed::audit",
            tracing::Level::DEBUG,
            provider = PROVIDER_NAME,
            exit_code = output.status.code().unwrap_or(-1),
            stdout_len = stdout.len(),
            stdout_preview = %crate::logging::preview_text(&stdout, 1200),
            stderr_len = stderr.len(),
            stderr_preview = %crate::logging::preview_text(&stderr, 1200),
            "Claude CLI process completed"
        );

        if !output.status.success() {
            let code = output.status.code().unwrap_or(-1);
            let mut details = Vec::new();
            if let Some(stdout_error) = extract_error_detail(&stdout) {
                details.push(format!("stdout: {}", clipped(&stdout_error)));
            }
            if !stderr.trim().is_empty() {
                details.push(format!("stderr: {}", clipped(&stderr)));
            }
            return Err(LlmError::RequestFailed {
                provider: PROVIDER_NAME.to_string(),
                reason: if details.is_empty() {
                    format!("claude command failed with exit code {}", code)
                } else {
                    format!(
                        "claude command failed with exit code {}: {}",
                        code,
                        details.join(" | ")
                    )
                },
            });
        }

        let parsed = parse_json_response(&stdout)?;
        tracing::event!(
            target: "clawed::audit",
            tracing::Level::DEBUG,
            provider = PROVIDER_NAME,
            parsed_input_tokens = parsed.input_tokens,
            parsed_output_tokens = parsed.output_tokens,
            parsed_content_len = parsed.content.len(),
            parsed_content_preview = %crate::logging::preview_text(&parsed.content, 1200),
            "Parsed Claude CLI JSON response"
        );
        Ok(parsed)
    }
}

#[async_trait]
impl LlmProvider for ClaudeCliProvider {
    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let parsed = self.run_completion(&request.messages, &[]).await?;
        Ok(CompletionResponse {
            content: parsed.content,
            input_tokens: parsed.input_tokens,
            output_tokens: parsed.output_tokens,
        })
    }

    async fn complete_with_tools(
        &self,
        request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        let parsed = self
            .run_completion(&request.messages, &request.tools)
            .await?;
        Ok(ToolCompletionResponse {
            content: Some(parsed.content),
            tool_calls: Vec::new(),
            input_tokens: parsed.input_tokens,
            output_tokens: parsed.output_tokens,
        })
    }
}

fn build_prompt(messages: &[ChatMessage], tools: &[ToolDefinition]) -> String {
    let mut out = String::new();
    out.push_str(
        "You are Clawed Agent operating through a text-only bridge.\n\
         Continue the conversation as the assistant and answer the latest user request.\n\
         Tool execution is disabled in this backend.\n",
    );

    if !tools.is_empty() {
        out.push_str("\nTools available to the caller (for context only):\n");
        for tool in tools {
            out.push_str("- ");
            out.push_str(&tool.name);
            out.push_str(": ");
            out.push_str(&tool.description);
            out.push('\n');
        }
    }

    out.push_str("\nConversation transcript:\n");
    for msg in messages {
        let role = match msg.role {
            Role::System => "SYSTEM",
            Role::User => "USER",
            Role::Assistant => "ASSISTANT",
            Role::Tool => "TOOL",
        };

        out.push('\n');
        out.push('[');
        out.push_str(role);
        out.push(']');
        if let Some(name) = &msg.name {
            out.push_str(" name=");
            out.push_str(name);
        }
        if let Some(tool_call_id) = &msg.tool_call_id {
            out.push_str(" tool_call_id=");
            out.push_str(tool_call_id);
        }
        out.push('\n');

        if let Some(tool_calls) = &msg.tool_calls {
            let tool_calls_json =
                serde_json::to_string_pretty(tool_calls).unwrap_or_else(|_| "[]".to_string());
            out.push_str("Tool calls:\n");
            out.push_str(&tool_calls_json);
            out.push('\n');
        }

        if msg.content.is_empty() {
            out.push_str("(empty)\n");
        } else {
            out.push_str(&msg.content);
            if !msg.content.ends_with('\n') {
                out.push('\n');
            }
        }
    }

    out.push_str("\nReturn only the assistant response content.");
    out
}

fn parse_json_response(stdout: &str) -> Result<ParsedOutput, LlmError> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Err(LlmError::RequestFailed {
            provider: PROVIDER_NAME.to_string(),
            reason: "claude command returned empty stdout".to_string(),
        });
    }

    let payload: Value = serde_json::from_str(trimmed).map_err(|e| LlmError::RequestFailed {
        provider: PROVIDER_NAME.to_string(),
        reason: format!("failed to parse claude JSON output: {}", e),
    })?;

    if payload
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        let detail = extract_primary_text(&payload).unwrap_or_else(|| "unknown error".to_string());
        return Err(LlmError::RequestFailed {
            provider: PROVIDER_NAME.to_string(),
            reason: format!("claude returned an error result: {}", detail),
        });
    }

    let content = extract_primary_text(&payload).ok_or_else(|| LlmError::RequestFailed {
        provider: PROVIDER_NAME.to_string(),
        reason: "claude JSON output did not include a result string".to_string(),
    })?;

    let input_tokens = extract_tokens(&payload, "input_tokens")
        .or_else(|| extract_tokens(&payload, "inputTokens"))
        .unwrap_or(0);
    let output_tokens = extract_tokens(&payload, "output_tokens")
        .or_else(|| extract_tokens(&payload, "outputTokens"))
        .unwrap_or(0);

    Ok(ParsedOutput {
        content,
        input_tokens,
        output_tokens,
    })
}

fn extract_primary_text(payload: &Value) -> Option<String> {
    for key in ["result", "content", "text", "output_text"] {
        if let Some(value) = payload.get(key).and_then(Value::as_str) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }

    if let Some(content) = payload.get("content").and_then(Value::as_array) {
        for item in content {
            if let Some(text) = item.get("text").and_then(Value::as_str) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
    }

    None
}

fn extract_tokens(payload: &Value, key: &str) -> Option<u32> {
    payload
        .get("usage")
        .and_then(|usage| usage.get(key))
        .and_then(value_to_u32)
}

fn extract_error_detail(stdout: &str) -> Option<String> {
    let payload: Value = serde_json::from_str(stdout.trim()).ok()?;
    extract_primary_text(&payload)
}

fn normalize_model_alias(model: &str) -> String {
    let lower = model.to_ascii_lowercase();
    match lower.as_str() {
        "opus4.6" | "opus-4.6" | "opus_4_6" => "claude-opus-4-6".to_string(),
        _ => model.to_string(),
    }
}

fn value_to_u32(value: &Value) -> Option<u32> {
    if let Some(n) = value.as_u64() {
        return Some(n.min(u32::MAX as u64) as u32);
    }
    if let Some(n) = value.as_i64() {
        if n >= 0 {
            return Some((n as u64).min(u32::MAX as u64) as u32);
        }
    }
    if let Some(n) = value.as_f64() {
        if n.is_finite() && n >= 0.0 {
            return Some((n as u64).min(u32::MAX as u64) as u32);
        }
    }
    if let Some(text) = value.as_str() {
        return text
            .parse::<u64>()
            .ok()
            .map(|n| n.min(u32::MAX as u64) as u32);
    }
    None
}

fn clipped(s: &str) -> String {
    let mut value = s.trim().to_string();
    if value.len() > ERROR_SNIPPET_LIMIT {
        value.truncate(ERROR_SNIPPET_LIMIT);
        value.push_str("... (truncated)");
    }
    value
}

fn shell_quote(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string();
    }

    if s.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || matches!(
                c,
                '_' | '-' | '.' | '/' | ':' | '=' | ',' | '+' | '@' | '%' | '^'
            )
    }) {
        return s.to_string();
    }

    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::provider::ChatMessage;

    #[test]
    fn parses_claude_result_shape() {
        let json = r#"{
            "type":"result",
            "subtype":"success",
            "is_error":false,
            "result":"hello",
            "usage":{"input_tokens":12,"output_tokens":34}
        }"#;

        let parsed = parse_json_response(json).expect("should parse");
        assert_eq!(parsed.content, "hello");
        assert_eq!(parsed.input_tokens, 12);
        assert_eq!(parsed.output_tokens, 34);
    }

    #[test]
    fn parses_fallback_content_array_shape() {
        let json = r#"{
            "type":"result",
            "is_error":false,
            "content":[{"type":"text","text":"fallback"}],
            "usage":{"inputTokens":1,"outputTokens":2}
        }"#;

        let parsed = parse_json_response(json).expect("should parse");
        assert_eq!(parsed.content, "fallback");
        assert_eq!(parsed.input_tokens, 1);
        assert_eq!(parsed.output_tokens, 2);
    }

    #[test]
    fn errors_on_missing_result_text() {
        let json = r#"{"type":"result","is_error":false}"#;
        let err = parse_json_response(json).expect_err("should fail");
        assert!(
            err.to_string().contains("did not include a result string"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn includes_role_tags_in_prompt() {
        let messages = vec![
            ChatMessage::system("sys"),
            ChatMessage::user("hello"),
            ChatMessage::assistant("world"),
        ];
        let prompt = build_prompt(&messages, &[]);
        assert!(prompt.contains("[SYSTEM]"));
        assert!(prompt.contains("[USER]"));
        assert!(prompt.contains("[ASSISTANT]"));
    }

    #[test]
    fn normalizes_opus_alias() {
        assert_eq!(normalize_model_alias("opus4.6"), "claude-opus-4-6");
        assert_eq!(normalize_model_alias("OpUs-4.6"), "claude-opus-4-6");
        assert_eq!(normalize_model_alias("claude-opus-4-6"), "claude-opus-4-6");
    }

    #[test]
    fn shell_quote_handles_spaces_and_quotes() {
        assert_eq!(shell_quote("simple"), "simple");
        assert_eq!(shell_quote("has space"), "'has space'");
        assert_eq!(shell_quote("a'b"), "'a'\"'\"'b'");
    }
}
