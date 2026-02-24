//! Shell execution tool with security checks.

use std::collections::HashSet;
use std::sync::LazyLock;
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::error::ToolError;
use crate::tools::{Tool, ToolContext, ToolOutput, require_str};

const MAX_OUTPUT_SIZE: usize = 64 * 1024;
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

static BLOCKED_COMMANDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        "rm -rf /",
        "rm -rf /*",
        ":(){ :|:& };:",
        "dd if=/dev/zero",
        "mkfs",
        "chmod -R 777 /",
        "> /dev/sda",
        "curl | sh",
        "wget | sh",
        "curl | bash",
        "wget | bash",
    ])
});

static DANGEROUS_PATTERNS: LazyLock<Vec<&'static str>> = LazyLock::new(|| {
    vec![
        "sudo ",
        "doas ",
        " | sh",
        " | bash",
        " | zsh",
        "eval ",
        "$(curl",
        "$(wget",
        "/etc/passwd",
        "/etc/shadow",
        "~/.ssh",
        ".bash_history",
        "id_rsa",
    ]
});

/// Environment variables safe to forward to child processes.
const SAFE_ENV_VARS: &[&str] = &[
    "PATH", "HOME", "USER", "LOGNAME", "SHELL", "TERM", "COLORTERM",
    "LANG", "LC_ALL", "LC_CTYPE", "LC_MESSAGES",
    "PWD", "TMPDIR", "TMP", "TEMP",
    "XDG_RUNTIME_DIR", "XDG_DATA_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME",
    "CARGO_HOME", "RUSTUP_HOME",
    "NODE_PATH", "NPM_CONFIG_PREFIX",
    "EDITOR", "VISUAL",
];

/// Detect injection patterns in commands.
fn detect_injection(command: &str) -> Option<String> {
    let lower = command.to_lowercase();

    // Base64 decode piped to shell
    if (lower.contains("base64") && (lower.contains("| sh") || lower.contains("| bash")))
        || lower.contains("base64 -d")
        || lower.contains("base64 --decode")
    {
        return Some("base64 decode piped to shell".to_string());
    }

    // DNS exfiltration
    if lower.contains("nslookup") || lower.contains("dig ") || lower.contains("host ") {
        if lower.contains("$(") || lower.contains("`") {
            return Some("potential DNS exfiltration".to_string());
        }
    }

    // Netcat/reverse shells
    for pattern in &["nc ", "ncat ", "netcat ", "socat "] {
        if lower.contains(pattern) {
            return Some(format!("network tool detected: {}", pattern.trim()));
        }
    }

    None
}

/// Shell command execution tool.
pub struct ShellTool;

impl ShellTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Execute a shell command. Returns stdout, stderr, and exit code. \
         Commands run with a scrubbed environment (no API keys or secrets). \
         Use for running builds, tests, git operations, or other CLI tasks."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                },
                "working_dir": {
                    "type": "string",
                    "description": "Working directory for the command (optional)"
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "Timeout in seconds (default: 120)"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &ToolContext,
    ) -> Result<ToolOutput, ToolError> {

        let command = require_str(&params, "command")?;

        // Check blocked commands
        if BLOCKED_COMMANDS.contains(command) {
            return Err(ToolError::ExecutionFailed(format!(
                "Command is blocked for safety: {}",
                command
            )));
        }

        // Check dangerous patterns
        let lower = command.to_lowercase();
        for pattern in DANGEROUS_PATTERNS.iter() {
            if lower.contains(pattern) {
                tracing::warn!(command, pattern, "Dangerous command pattern detected");
            }
        }

        // Check for injection
        if let Some(reason) = detect_injection(command) {
            return Err(ToolError::ExecutionFailed(format!(
                "Command injection detected: {}",
                reason
            )));
        }

        let working_dir = params
            .get("working_dir")
            .and_then(|v| v.as_str())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| ctx.working_dir.clone());

        let timeout = params
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .map(Duration::from_secs)
            .unwrap_or(DEFAULT_TIMEOUT);

        // Build command with scrubbed environment
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(command);
        cmd.current_dir(&working_dir);
        cmd.env_clear();

        // Forward only safe environment variables
        for var in SAFE_ENV_VARS {
            if let Ok(val) = std::env::var(var) {
                cmd.env(var, val);
            }
        }

        // Forward extra env from context
        for (k, v) in &ctx.extra_env {
            cmd.env(k, v);
        }

        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to spawn command: {}", e))
        })?;

        // Read output with timeout
        let result = tokio::time::timeout(timeout, async {
            let mut stdout_buf = Vec::new();
            let mut stderr_buf = Vec::new();

            if let Some(ref mut stdout) = child.stdout {
                let _ = stdout.read_to_end(&mut stdout_buf).await;
            }
            if let Some(ref mut stderr) = child.stderr {
                let _ = stderr.read_to_end(&mut stderr_buf).await;
            }

            let status = child.wait().await;
            (stdout_buf, stderr_buf, status)
        })
        .await;

        match result {
            Ok((stdout_buf, stderr_buf, status)) => {
                let exit_code = status
                    .map(|s| s.code().unwrap_or(-1))
                    .unwrap_or(-1);

                let mut stdout = String::from_utf8_lossy(&stdout_buf).to_string();
                let stderr = String::from_utf8_lossy(&stderr_buf).to_string();

                // Truncate if too large
                if stdout.len() > MAX_OUTPUT_SIZE {
                    stdout.truncate(MAX_OUTPUT_SIZE);
                    stdout.push_str("\n... (output truncated)");
                }

                let output = if stderr.is_empty() {
                    format!("[exit code: {}]\n{}", exit_code, stdout)
                } else {
                    let mut stderr_trimmed = stderr;
                    if stderr_trimmed.len() > MAX_OUTPUT_SIZE / 2 {
                        stderr_trimmed.truncate(MAX_OUTPUT_SIZE / 2);
                        stderr_trimmed.push_str("\n... (stderr truncated)");
                    }
                    format!(
                        "[exit code: {}]\n{}\n[stderr]\n{}",
                        exit_code, stdout, stderr_trimmed
                    )
                };

                Ok(ToolOutput::text(output))
            }
            Err(_) => {
                let _ = child.kill().await;
                Err(ToolError::Timeout(timeout))
            }
        }
    }
}
