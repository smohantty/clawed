//! Minimal safety layer: output sanitization and injection detection.

/// Result of sanitizing tool output.
pub struct SanitizedOutput {
    pub content: String,
    pub was_modified: bool,
}

/// Basic injection patterns to detect in tool output.
const INJECTION_PATTERNS: &[&str] = &[
    "ignore previous",
    "ignore all previous",
    "forget everything",
    "you are now",
    "system:",
    "<|",
    "|>",
    "[INST]",
    "[/INST]",
    "new instructions",
];

/// Sanitize tool output for safe inclusion in LLM context.
pub fn sanitize_tool_output(tool_name: &str, output: &str) -> SanitizedOutput {
    let mut content = output.to_string();
    let mut was_modified = false;

    // Check for injection patterns (warn but don't block)
    let lower = content.to_lowercase();
    for pattern in INJECTION_PATTERNS {
        if lower.contains(pattern) {
            tracing::warn!(
                tool = tool_name,
                pattern,
                "Potential injection pattern detected in tool output"
            );
            was_modified = true;
        }
    }

    // Truncate extremely large outputs
    const MAX_TOOL_OUTPUT: usize = 128 * 1024;
    if content.len() > MAX_TOOL_OUTPUT {
        content.truncate(MAX_TOOL_OUTPUT);
        content.push_str("\n... (output truncated for safety)");
        was_modified = true;
    }

    SanitizedOutput {
        content,
        was_modified,
    }
}

/// Wrap tool output in XML tags for LLM context.
pub fn wrap_for_llm(tool_name: &str, content: &str, was_modified: bool) -> String {
    let sanitized_attr = if was_modified {
        " sanitized=\"true\""
    } else {
        ""
    };
    format!(
        "<tool_output name=\"{}\"{}>\n{}\n</tool_output>",
        tool_name, sanitized_attr, content
    )
}
