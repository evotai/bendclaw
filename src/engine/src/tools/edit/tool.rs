//! Edit tool — surgical search/replace edits on files.
//!
//! This is the most important tool for coding agents. Instead of rewriting
//! entire files, the agent specifies exact text to find and replace.

use async_trait::async_trait;

use super::diff;
use super::matching;
use super::matching::MatchError;
use super::normalize;
use crate::types::*;

/// Surgical file editing via exact text search/replace.
pub struct EditFileTool {
    disallow_message: Option<String>,
}

impl Default for EditFileTool {
    fn default() -> Self {
        Self::new()
    }
}

impl EditFileTool {
    pub fn new() -> Self {
        Self {
            disallow_message: None,
        }
    }

    /// Mark this tool as disallowed. `execute()` will return the given message
    /// instead of performing the edit.
    pub fn disallow(mut self, message: impl Into<String>) -> Self {
        self.disallow_message = Some(message.into());
        self
    }
}

#[async_trait]
impl AgentTool for EditFileTool {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn label(&self) -> &str {
        "Edit File"
    }

    fn description(&self) -> &str {
        "Perform exact string replacements in files.\n\
         \n\
         Usage:\n\
         - You must use read_file at least once in the conversation before editing. \
         This tool will error if you attempt an edit without reading the file first.\n\
         - When editing text from read_file output, ensure you preserve the exact indentation \
         (tabs/spaces) as it appears AFTER the line number prefix. The line number prefix format is: \
         line number + pipe. Everything after that is the actual file content to match. \
         Never include any part of the line number prefix in old_text or new_text.\n\
         - ALWAYS prefer editing existing files in the codebase. NEVER write new files unless \
         explicitly required.\n\
         - The edit will FAIL if old_text is not unique in the file. Either provide a larger \
         string with more surrounding context to make it unique, or use replace_all to change \
         every instance of old_text.\n\
         - Use replace_all for replacing and renaming exact strings across the file. This is useful \
         for renaming a variable, for instance. Note: replace_all only supports exact matches \
         and will not use fallback matching.\n\
         - Use this tool instead of shell sed/awk for file modifications."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path to edit"
                },
                "old_text": {
                    "type": "string",
                    "description": "Exact text to find (must match exactly, including whitespace)"
                },
                "new_text": {
                    "type": "string",
                    "description": "Text to replace it with"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace all exact occurrences of old_text (default false)"
                }
            },
            "required": ["path", "old_text", "new_text"]
        })
    }

    fn preview_command(&self, params: &serde_json::Value) -> Option<String> {
        let path = params["path"].as_str()?;
        let replace_all = params["replace_all"].as_bool().unwrap_or(false);
        if replace_all {
            Some(format!("sed -i 's/<old>/<new>/g' {path}"))
        } else {
            Some(format!("sed -i 's/<old>/<new>/' {path}"))
        }
    }

    fn is_concurrency_safe(&self) -> bool {
        false
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        if let Some(msg) = &self.disallow_message {
            return Err(ToolError::Failed(format!("Error: {msg}")));
        }

        let path_str = params["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs("missing 'path' parameter".into()))?;
        let old_text = params["old_text"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs("missing 'old_text' parameter".into()))?;
        let new_text = params["new_text"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs("missing 'new_text' parameter".into()))?;
        let replace_all = params["replace_all"].as_bool().unwrap_or(false);

        let path = ctx.path_guard.resolve_path(&ctx.cwd, path_str)?;

        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }

        // Read file bytes and validate UTF-8
        let bytes = tokio::fs::read(&path).await.map_err(|e| {
            ToolError::Failed(format!(
                "Cannot read {path_str}: {e}. Use write_file to create new files."
            ))
        })?;
        let raw = String::from_utf8(bytes).map_err(|_| {
            ToolError::Failed(format!(
                "Cannot edit {path_str}: only UTF-8 text files are supported."
            ))
        })?;

        // Strip BOM, detect line endings, normalize to LF
        let (bom, content_raw) = normalize::strip_utf8_bom(&raw);
        let line_ending = normalize::detect_line_ending(content_raw);
        let content_lf = normalize::normalize_to_lf(content_raw);
        let old_text_lf = normalize::normalize_to_lf(old_text);
        let new_text_lf = normalize::normalize_to_lf(new_text);

        if old_text_lf.is_empty() {
            return Err(ToolError::Failed("old_text must not be empty.".into()));
        }

        // Branch: replace_all uses exact match only; single replace uses
        // the tiered fallback in resolve_unique_match.
        let (new_content_lf, match_kind, replacement_count) = if replace_all {
            let count = content_lf.matches(&old_text_lf).count();
            if count == 0 {
                let hint = matching::find_similar_text(&content_lf, &old_text_lf);
                let suffix = match hint {
                    Some(similar) => format!(
                        "\n\nDid you mean:\n```\n{similar}\n```\n\
                         replace_all requires an exact match."
                    ),
                    None => "\n\nreplace_all requires an exact match. \
                         Use read_file to see the current file contents."
                        .into(),
                };
                return Err(ToolError::Failed(format!(
                    "old_text not found in {path_str}.{suffix}"
                )));
            }
            let replaced = content_lf.replace(&old_text_lf, &new_text_lf);
            (replaced, "exact", count)
        } else {
            let resolved =
                matching::resolve_unique_match(&content_lf, &old_text_lf).map_err(|e| match e {
                    MatchError::EmptyOldText => {
                        ToolError::Failed("old_text must not be empty.".into())
                    }
                    MatchError::NotFound => {
                        let hint = matching::find_similar_text(&content_lf, &old_text_lf);
                        let suffix = match hint {
                            Some(similar) => format!(
                                "\n\nDid you mean:\n```\n{similar}\n```\n\
                                 Make sure old_text matches the current file content, \
                                 including indentation."
                            ),
                            None => "\n\nTip: Use read_file to see the current file contents, \
                                 then copy the exact text you want to replace."
                                .into(),
                        };
                        ToolError::Failed(format!("old_text not found in {path_str}.{suffix}"))
                    }
                    MatchError::NotUnique { count } => ToolError::Failed(format!(
                        "old_text matches {count} locations in {path_str}. \
                         Include more surrounding context to make the match unique, \
                         or set replace_all to true to replace all occurrences."
                    )),
                })?;

            let replaced = content_lf.replacen(&resolved.actual_old_text, &new_text_lf, 1);
            let kind = resolved.kind.as_str();
            (replaced, kind, 1)
        };

        // No-change detection
        if new_content_lf == content_lf {
            return Err(ToolError::Failed(format!(
                "No changes made to {path_str}. The replacement produced identical content."
            )));
        }

        // Generate diff (for details only, not sent to LLM)
        let diff_result = diff::unified_diff(&content_lf, &new_content_lf, path_str);

        // Restore BOM + original line endings and write back
        let final_content = format!(
            "{}{}",
            bom,
            normalize::restore_line_endings(&new_content_lf, line_ending)
        );
        tokio::fs::write(&path, &final_content)
            .await
            .map_err(|e| ToolError::Failed(format!("Cannot write {path_str}: {e}")))?;

        Ok(ToolResult {
            content: vec![Content::Text {
                text: format!("Updated {path_str}."),
            }],
            details: serde_json::json!({
                "path": path_str,
                "match_kind": match_kind,
                "replace_all": replace_all,
                "replacement_count": replacement_count,
                "diff": diff_result.unified,
                "first_changed_line": diff_result.first_changed_line,
                "added_lines": diff_result.added_lines,
                "removed_lines": diff_result.removed_lines,
            }),
            retention: Retention::Normal,
        })
    }
}
