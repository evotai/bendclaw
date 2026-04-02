//! Content sanitization for skill descriptions and documentation.
//!
//! Detects and removes prompt-injection patterns before skill content
//! is exposed to the LLM (via tool descriptions or `skill_read` output).

/// A single detected suspicious pattern.
#[derive(Debug, Clone)]
pub struct SanitizeWarning {
    pub pattern: &'static str,
    pub description: &'static str,
}

/// Result of sanitizing a piece of text.
#[derive(Debug, Clone)]
pub struct SanitizeResult {
    pub content: String,
    pub warnings: Vec<SanitizeWarning>,
}

struct Pattern {
    /// Case-insensitive substring to detect.
    needle: &'static str,
    label: &'static str,
    description: &'static str,
}

const PATTERNS: &[Pattern] = &[
    Pattern {
        needle: "\"arguments\":",
        label: "tool_call_json",
        description: "JSON tool-call syntax",
    },
    Pattern {
        needle: "<tool_call>",
        label: "tool_call_xml",
        description: "XML tool-call syntax",
    },
    Pattern {
        needle: "you are now",
        label: "identity_override",
        description: "identity override attempt",
    },
    Pattern {
        needle: "ignore previous",
        label: "ignore_instructions",
        description: "instruction override attempt",
    },
    Pattern {
        needle: "system: ",
        label: "system_role_injection",
        description: "system role injection",
    },
    Pattern {
        needle: "wallet.json",
        label: "sensitive_file_wallet",
        description: "sensitive file reference (wallet)",
    },
    Pattern {
        needle: ".env",
        label: "sensitive_file_env",
        description: "sensitive file reference (.env)",
    },
    Pattern {
        needle: "private_key",
        label: "sensitive_file_key",
        description: "sensitive file reference (private key)",
    },
    Pattern {
        needle: "privatekey",
        label: "sensitive_file_key2",
        description: "sensitive file reference (private key)",
    },
];

/// Lowercase only ASCII characters, preserving byte-length alignment
/// with the original string. This avoids the Unicode case-fold length
/// mismatch (e.g. İ → i̇ changes byte count) while still matching
/// our ASCII-only needles.
fn ascii_lowercase(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii() {
                c.to_ascii_lowercase()
            } else {
                c
            }
        })
        .collect()
}

fn sanitize(input: &str) -> SanitizeResult {
    let mut content = input.to_string();
    let mut warnings = Vec::new();
    for p in PATTERNS {
        let lower = ascii_lowercase(&content);
        if !lower.contains(p.needle) {
            continue;
        }

        crate::kernel::skills::diagnostics::log_skill_sanitizer_detected(
            p.label,
            p.description,
            p.needle,
        );

        warnings.push(SanitizeWarning {
            pattern: p.label,
            description: p.description,
        });

        let replacement = format!("[REMOVED:{}]", p.label);
        let needle_len = p.needle.len();
        let mut result = String::with_capacity(content.len());
        let mut search_start = 0;
        let mut match_count = 0u32;

        while search_start + needle_len <= lower.len() {
            if let Some(pos) = lower[search_start..].find(p.needle) {
                let abs_pos = search_start + pos;
                result.push_str(&content[search_start..abs_pos]);
                result.push_str(&replacement);
                search_start = abs_pos + needle_len;
                match_count += 1;
            } else {
                break;
            }
        }
        result.push_str(&content[search_start..]);
        content = result;

        crate::kernel::skills::diagnostics::log_skill_sanitizer_replaced(p.label, match_count);
    }

    SanitizeResult { content, warnings }
}

/// Sanitize skill content (SKILL.md body) before exposing to the LLM.
pub fn sanitize_skill_content(content: &str) -> SanitizeResult {
    sanitize(content)
}

/// Sanitize a skill description before using it as a tool description.
pub fn sanitize_skill_description(description: &str) -> SanitizeResult {
    sanitize(description)
}
