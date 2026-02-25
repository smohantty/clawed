//! Logging helpers for structured audit output.

use std::sync::LazyLock;

static AUDIT_FULL_PAYLOADS: LazyLock<bool> = LazyLock::new(|| {
    std::env::var("CLAWED_AUDIT_FULL_PAYLOADS")
        .map(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
});

/// Whether full payload bodies should be logged in audit events.
pub fn audit_full_payloads_enabled() -> bool {
    *AUDIT_FULL_PAYLOADS
}

/// Build a preview string with truncation unless full payload logging is enabled.
pub fn preview_text(text: &str, max_chars: usize) -> String {
    if audit_full_payloads_enabled() {
        return text.to_string();
    }

    // Keep default audit lines compact and grep-friendly.
    let normalized = text.replace('\r', "\\r").replace('\n', "\\n");
    if normalized.chars().count() <= max_chars {
        return normalized;
    }

    let mut preview = String::new();
    for c in normalized.chars().take(max_chars) {
        preview.push(c);
    }
    preview.push_str("... <truncated>");
    preview
}
