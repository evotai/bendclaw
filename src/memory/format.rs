//! Memory formatting — pure function for prompt injection.

use super::store::MemoryEntry;

/// Format memory entries into a prompt section, respecting a character budget.
///
/// Returns `None` if:
/// - `entries` is empty
/// - `budget` is too small to hold even the header + one entry
///
/// This is a pure function with no I/O — easy to test.
pub fn format_for_prompt(entries: &[MemoryEntry], budget: usize) -> Option<String> {
    if entries.is_empty() || budget < 20 {
        return None;
    }

    let header = "## Memory\n";
    let mut buf = String::from(header);

    for entry in entries {
        let line = format!("- {}: {}\n", entry.key, entry.content);
        if buf.len() + line.len() > budget {
            break;
        }
        buf.push_str(&line);
    }

    // No entries fit — don't emit an empty section.
    if buf.len() <= header.len() {
        return None;
    }

    // Final safety truncation on char boundary.
    if buf.len() > budget {
        let mut end = budget;
        while end > 0 && !buf.is_char_boundary(end) {
            end -= 1;
        }
        buf.truncate(end);
    }

    Some(buf)
}
