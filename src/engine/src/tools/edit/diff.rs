//! Unified diff generation for edit results.
//!
//! Adapted from claw's `open-agent-sdk-rust/src/tools/diff.rs`.
//! All functions are pure — no IO, no side effects.

/// Result of generating a unified diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffResult {
    /// Unified diff string.
    pub unified: String,
    /// Line number (1-based) of the first change in the **new** file content.
    pub first_changed_line: Option<usize>,
    /// Number of lines added.
    pub added_lines: usize,
    /// Number of lines removed.
    pub removed_lines: usize,
}

/// Generate a unified diff between `old` and `new` content, with 3 context lines.
pub fn unified_diff(old: &str, new: &str, filename: &str) -> DiffResult {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();

    let hunks = compute_hunks(&old_lines, &new_lines);

    let mut added_lines = 0;
    let mut removed_lines = 0;
    let mut first_changed_line: Option<usize> = None;

    let mut result = String::new();
    result.push_str(&format!("--- a/{filename}\n"));
    result.push_str(&format!("+++ b/{filename}\n"));

    for hunk in &hunks {
        result.push_str(&format!(
            "@@ -{},{} +{},{} @@\n",
            hunk.old_start + 1,
            hunk.old_count,
            hunk.new_start + 1,
            hunk.new_count
        ));

        let mut new_line_num = hunk.new_start + 1;
        for line in &hunk.lines {
            match line {
                DiffLine::Context(s) => {
                    result.push(' ');
                    result.push_str(s);
                    result.push('\n');
                    new_line_num += 1;
                }
                DiffLine::Added(s) => {
                    result.push('+');
                    result.push_str(s);
                    result.push('\n');
                    added_lines += 1;
                    if first_changed_line.is_none() {
                        first_changed_line = Some(new_line_num);
                    }
                    new_line_num += 1;
                }
                DiffLine::Removed(s) => {
                    result.push('-');
                    result.push_str(s);
                    result.push('\n');
                    removed_lines += 1;
                    if first_changed_line.is_none() {
                        first_changed_line = Some(new_line_num);
                    }
                }
            }
        }
    }

    DiffResult {
        unified: result,
        first_changed_line,
        added_lines,
        removed_lines,
    }
}

// ---------------------------------------------------------------------------
// Internal types and algorithms (adapted from claw)
// ---------------------------------------------------------------------------

enum DiffLine<'a> {
    Context(&'a str),
    Added(&'a str),
    Removed(&'a str),
}

struct Hunk<'a> {
    old_start: usize,
    old_count: usize,
    new_start: usize,
    new_count: usize,
    lines: Vec<DiffLine<'a>>,
}

fn compute_hunks<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<Hunk<'a>> {
    let n = old.len();
    let m = new.len();
    let context_lines = 3;

    // Compute edit script via LCS
    let lcs = compute_lcs(old, new);
    let mut edits: Vec<DiffLine<'a>> = Vec::new();
    let mut old_idx = 0;
    let mut new_idx = 0;
    let mut lcs_idx = 0;

    while old_idx < n || new_idx < m {
        if lcs_idx < lcs.len() && old_idx == lcs[lcs_idx].0 && new_idx == lcs[lcs_idx].1 {
            edits.push(DiffLine::Context(old[old_idx]));
            old_idx += 1;
            new_idx += 1;
            lcs_idx += 1;
        } else if old_idx < n && (lcs_idx >= lcs.len() || old_idx < lcs[lcs_idx].0) {
            edits.push(DiffLine::Removed(old[old_idx]));
            old_idx += 1;
        } else if new_idx < m {
            edits.push(DiffLine::Added(new[new_idx]));
            new_idx += 1;
        }
    }

    // Group edits into hunks
    let mut hunks = Vec::new();
    let mut i = 0;

    while i < edits.len() {
        // Find next change
        while i < edits.len() && matches!(edits[i], DiffLine::Context(_)) {
            i += 1;
        }
        if i >= edits.len() {
            break;
        }

        // Start hunk with context before
        let context_start = i.saturating_sub(context_lines);

        let mut hunk_lines = Vec::new();

        // Calculate starting positions
        let mut oi = 0;
        let mut ni = 0;
        for edit in edits.iter().take(context_start) {
            match edit {
                DiffLine::Context(_) => {
                    oi += 1;
                    ni += 1;
                }
                DiffLine::Removed(_) => oi += 1,
                DiffLine::Added(_) => ni += 1,
            }
        }
        let old_start = oi;
        let new_start = ni;

        // Add context before
        let mut j = context_start;
        while j < i {
            if let DiffLine::Context(s) = &edits[j] {
                hunk_lines.push(DiffLine::Context(s));
            }
            j += 1;
        }

        // Add changes and merge close hunks
        let mut consecutive_context = 0;
        while j < edits.len() {
            match &edits[j] {
                DiffLine::Context(s) => {
                    consecutive_context += 1;
                    if consecutive_context > context_lines * 2 {
                        // End hunk, remove excess trailing context
                        let to_remove = consecutive_context - context_lines;
                        for _ in 0..to_remove {
                            hunk_lines.pop();
                        }
                        break;
                    }
                    hunk_lines.push(DiffLine::Context(s));
                }
                DiffLine::Added(s) => {
                    consecutive_context = 0;
                    hunk_lines.push(DiffLine::Added(s));
                }
                DiffLine::Removed(s) => {
                    consecutive_context = 0;
                    hunk_lines.push(DiffLine::Removed(s));
                }
            }
            j += 1;
        }

        // Trim trailing context beyond limit
        loop {
            let trailing = hunk_lines
                .iter()
                .rev()
                .take_while(|l| matches!(l, DiffLine::Context(_)))
                .count();
            if trailing > context_lines {
                hunk_lines.pop();
            } else {
                break;
            }
        }

        let mut old_count = 0;
        let mut new_count = 0;
        for line in &hunk_lines {
            match line {
                DiffLine::Context(_) => {
                    old_count += 1;
                    new_count += 1;
                }
                DiffLine::Removed(_) => old_count += 1,
                DiffLine::Added(_) => new_count += 1,
            }
        }

        if old_count > 0 || new_count > 0 {
            hunks.push(Hunk {
                old_start,
                old_count,
                new_start,
                new_count,
                lines: hunk_lines,
            });
        }

        i = j;
    }

    hunks
}

fn compute_lcs<'a>(old: &[&'a str], new: &[&'a str]) -> Vec<(usize, usize)> {
    let n = old.len();
    let m = new.len();

    if n == 0 || m == 0 {
        return Vec::new();
    }

    let mut dp = vec![vec![0u32; m + 1]; n + 1];

    for i in 1..=n {
        for j in 1..=m {
            if old[i - 1] == new[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
            }
        }
    }

    let mut result = Vec::new();
    let mut i = n;
    let mut j = m;

    while i > 0 && j > 0 {
        if old[i - 1] == new[j - 1] {
            result.push((i - 1, j - 1));
            i -= 1;
            j -= 1;
        } else if dp[i - 1][j] > dp[i][j - 1] {
            i -= 1;
        } else {
            j -= 1;
        }
    }

    result.reverse();
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_replace() {
        let old = "aaa\nbbb\nccc\n";
        let new = "aaa\nxxx\nccc\n";
        let d = unified_diff(old, new, "test.rs");
        assert_eq!(d.added_lines, 1);
        assert_eq!(d.removed_lines, 1);
        assert!(d.unified.contains("-bbb"));
        assert!(d.unified.contains("+xxx"));
    }

    #[test]
    fn multi_line_add() {
        let old = "aaa\nccc\n";
        let new = "aaa\nbbb\nccc\n";
        let d = unified_diff(old, new, "test.rs");
        assert_eq!(d.added_lines, 1);
        assert_eq!(d.removed_lines, 0);
        assert!(d.unified.contains("+bbb"));
    }

    #[test]
    fn multi_line_delete() {
        let old = "aaa\nbbb\nccc\n";
        let new = "aaa\nccc\n";
        let d = unified_diff(old, new, "test.rs");
        assert_eq!(d.added_lines, 0);
        assert_eq!(d.removed_lines, 1);
        assert!(d.unified.contains("-bbb"));
    }

    #[test]
    fn first_changed_line_correct() {
        let old = "line1\nline2\nline3\nline4\n";
        let new = "line1\nline2\nchanged\nline4\n";
        let d = unified_diff(old, new, "test.rs");
        // line3 → changed is at line 3 in the new file
        assert_eq!(d.first_changed_line, Some(3));
    }

    #[test]
    fn no_changes_empty_diff() {
        let content = "aaa\nbbb\n";
        let d = unified_diff(content, content, "test.rs");
        assert_eq!(d.added_lines, 0);
        assert_eq!(d.removed_lines, 0);
        assert_eq!(d.first_changed_line, None);
    }

    #[test]
    fn diff_header_format() {
        let d = unified_diff("a\n", "b\n", "foo.rs");
        assert!(d.unified.starts_with("--- a/foo.rs\n+++ b/foo.rs\n"));
    }
}
