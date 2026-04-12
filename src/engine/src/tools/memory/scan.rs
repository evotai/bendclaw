//! Lightweight security scan for memory content.
//!
//! Memory entries are injected into the system prompt on future sessions,
//! making them a potential vector for prompt injection and credential
//! exfiltration. This module provides a best-effort gate that rejects
//! obviously malicious content before it reaches disk.

/// Invisible unicode characters that can be used for injection.
const INVISIBLE_CHARS: &[char] = &[
    '\u{200b}', // zero-width space
    '\u{200c}', // zero-width non-joiner
    '\u{200d}', // zero-width joiner
    '\u{2060}', // word joiner
    '\u{feff}', // zero-width no-break space
    '\u{202a}', // left-to-right embedding
    '\u{202b}', // right-to-left embedding
    '\u{202c}', // pop directional formatting
    '\u{202d}', // left-to-right override
    '\u{202e}', // right-to-left override
];

/// Case-insensitive substring phrases and their threat labels.
const THREAT_PHRASES: &[(&[&str], &str)] = &[
    // Prompt injection — each inner slice is a sequence of words that must
    // appear consecutively (case-insensitive) in the content.
    (&["ignore", "previous", "instructions"], "prompt_injection"),
    (&["ignore", "all", "instructions"], "prompt_injection"),
    (&["ignore", "above", "instructions"], "prompt_injection"),
    (&["ignore", "prior", "instructions"], "prompt_injection"),
    (&["you", "are", "now"], "role_hijack"),
    (&["disregard", "your", "instructions"], "disregard_rules"),
    (&["disregard", "all", "instructions"], "disregard_rules"),
    (&["disregard", "any", "instructions"], "disregard_rules"),
    (&["disregard", "your", "rules"], "disregard_rules"),
    (&["disregard", "all", "rules"], "disregard_rules"),
    (&["disregard", "your", "guidelines"], "disregard_rules"),
    (&["system", "prompt", "override"], "sys_prompt_override"),
];

/// Exfiltration patterns: (command prefix, secret-related substrings).
const EXFIL_COMMANDS: &[(&str, &[&str])] = &[
    ("curl", &[
        "key",
        "token",
        "secret",
        "password",
        "credential",
        "api_key",
    ]),
    ("wget", &[
        "key",
        "token",
        "secret",
        "password",
        "credential",
        "api_key",
    ]),
];

const SECRET_FILE_PATTERNS: &[&str] = &[".env", "credentials", ".netrc", ".pgpass"];

/// Scan content for security threats.
///
/// Returns `Some(reason)` if the content is blocked, `None` if safe.
pub fn scan_content(content: &str) -> Option<String> {
    check_invisible_chars(content)
        .or_else(|| check_threat_phrases(content))
        .or_else(|| check_exfiltration(content))
}

fn check_invisible_chars(content: &str) -> Option<String> {
    for ch in INVISIBLE_CHARS {
        if content.contains(*ch) {
            return Some(format!(
                "Blocked: content contains invisible unicode character U+{:04X}. \
                 Memory entries must not contain hidden characters.",
                *ch as u32
            ));
        }
    }
    None
}

fn check_threat_phrases(content: &str) -> Option<String> {
    let lower = content.to_lowercase();
    for (phrase_words, label) in THREAT_PHRASES {
        if contains_word_sequence(&lower, phrase_words) {
            return Some(format!(
                "Blocked: content matches threat pattern '{label}'. \
                 Memory entries are injected into the system prompt \
                 and must not contain injection or exfiltration payloads.",
            ));
        }
    }
    None
}

fn check_exfiltration(content: &str) -> Option<String> {
    let lower = content.to_lowercase();
    for line in lower.lines() {
        let trimmed = line.trim();

        // Check curl/wget + secret variable patterns
        for (cmd, secrets) in EXFIL_COMMANDS {
            if trimmed.starts_with(cmd) || trimmed.contains(&format!(" {cmd} ")) {
                for secret in *secrets {
                    if trimmed.contains(secret) {
                        return Some(format!(
                            "Blocked: content matches threat pattern 'exfil_{cmd}'. \
                             Memory entries are injected into the system prompt \
                             and must not contain injection or exfiltration payloads.",
                        ));
                    }
                }
            }
        }

        // Check cat + secret file patterns
        if trimmed.starts_with("cat ") {
            for pattern in SECRET_FILE_PATTERNS {
                if trimmed.contains(pattern) {
                    return Some(
                        "Blocked: content matches threat pattern 'read_secrets'. \
                         Memory entries are injected into the system prompt \
                         and must not contain injection or exfiltration payloads."
                            .to_string(),
                    );
                }
            }
        }
    }
    None
}

/// Check if `haystack` contains all words in `words` appearing in order,
/// separated by whitespace (not necessarily adjacent).
fn contains_word_sequence(haystack: &str, words: &[&str]) -> bool {
    if words.is_empty() {
        return true;
    }
    let hay_words: Vec<&str> = haystack.split_whitespace().collect();
    let mut wi = 0;
    for hw in &hay_words {
        if *hw == words[wi] || hw.starts_with(words[wi]) {
            wi += 1;
            if wi == words.len() {
                return true;
            }
        }
    }
    false
}
