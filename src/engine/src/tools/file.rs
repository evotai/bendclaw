//! File tools — read and write files with safety limits.

use std::path::Path;

use async_trait::async_trait;
use base64::Engine;

use crate::types::*;

/// 20 MB limit for image files
const MAX_IMAGE_SIZE_BYTES: u64 = 20 * 1024 * 1024;

fn is_image_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .as_deref(),
        Some("jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp")
    )
}

fn get_image_mime_type(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .as_deref()
    {
        Some("jpg" | "jpeg") => Some("image/jpeg"),
        Some("png") => Some("image/png"),
        Some("webp") => Some("image/webp"),
        Some("gif") => Some("image/gif"),
        Some("bmp") => Some("image/bmp"),
        _ => None,
    }
}

/// Read a file's contents. Supports line range for large files.
pub struct ReadFileTool {
    /// Max file size to read (prevents OOM)
    pub max_bytes: usize,
    /// Allowed directory roots (empty = no restriction)
    pub allowed_paths: Vec<String>,
}

impl Default for ReadFileTool {
    fn default() -> Self {
        Self {
            max_bytes: 1024 * 1024, // 1MB
            allowed_paths: Vec::new(),
        }
    }
}

impl ReadFileTool {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl AgentTool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn label(&self) -> &str {
        "Read File"
    }

    fn description(&self) -> &str {
        "Read a text file from the local filesystem. You can access any file directly by using this tool.\n\
         \n\
         Usage:\n\
         - The path parameter should be an absolute path, not a relative path.\n\
         - Use this tool instead of shell cat/head/tail for reading files.\n\
         - Supports optional offset/limit for partial reads of large files.\n\
         - This tool can only read text files and images (jpg, png, webp, gif, bmp), not directories or binary files.\n\
         To list a directory, use list_files.\n\
         - If you read a file that exists but has empty contents you will receive a warning \
         in place of file contents."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path to read"
                },
                "offset": {
                    "type": "integer",
                    "description": "Starting line number (1-indexed, optional)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to return (optional)"
                }
            },
            "required": ["path"]
        })
    }

    fn preview_command(&self, params: &serde_json::Value) -> Option<String> {
        let path = params["path"].as_str()?;
        let offset = params["offset"].as_u64();
        let limit = params["limit"].as_u64();
        match (offset, limit) {
            (Some(off), Some(lim)) => {
                let end = off.saturating_add(lim).saturating_sub(1);
                Some(format!("sed -n '{},{}p' {}", off, end, path))
            }
            (Some(off), None) => Some(format!("sed -n '{},$p' {}", off, path)),
            (None, Some(lim)) => Some(format!("head -n {} {}", lim, path)),
            (None, None) => Some(format!("cat -n {}", path)),
        }
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let path = params["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs("missing 'path' parameter".into()))?;

        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }

        // Check file exists and size
        let metadata = tokio::fs::metadata(path)
            .await
            .map_err(|e| ToolError::Failed(format!("Cannot access {}: {}", path, e)))?;

        // Handle image files: read as binary, return base64-encoded Content::Image
        let file_path = Path::new(path);
        if is_image_file(file_path) {
            if metadata.len() > MAX_IMAGE_SIZE_BYTES {
                return Err(ToolError::Failed(format!(
                    "Image too large ({}MB, max 20MB)",
                    metadata.len() / (1024 * 1024)
                )));
            }
            let mime_type = get_image_mime_type(file_path)
                .ok_or_else(|| ToolError::Failed("Unknown image format".into()))?;
            let bytes = tokio::fs::read(path)
                .await
                .map_err(|e| ToolError::Failed(format!("Cannot read {}: {}", path, e)))?;
            let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
            return Ok(ToolResult {
                content: vec![Content::Image {
                    data,
                    mime_type: mime_type.to_string(),
                }],
                details: serde_json::json!({ "path": path, "bytes": bytes.len() }),
            });
        }

        // Text files: check size limit and apply line offset/limit
        if metadata.len() as usize > self.max_bytes {
            return Err(ToolError::Failed(format!(
                "File too large ({} bytes, max {}). Use offset/limit for partial reads.",
                metadata.len(),
                self.max_bytes
            )));
        }

        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| ToolError::Failed(format!("Cannot read {}: {}", path, e)))?;

        let offset = params["offset"].as_u64().map(|v| v.max(1) as usize);
        let limit = params["limit"].as_u64().map(|v| v as usize);

        // Always show line numbers — helps agent reference exact lines for edit_file
        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();

        let (start, end) = match (offset, limit) {
            (Some(off), Some(lim)) => {
                let s = (off - 1).min(total);
                (s, (s + lim).min(total))
            }
            (Some(off), None) => {
                let s = (off - 1).min(total);
                (s, total)
            }
            (None, Some(lim)) => (0, lim.min(total)),
            (None, None) => (0, total),
        };

        let numbered: Vec<String> = lines[start..end]
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{:>4} | {}", start + i + 1, line))
            .collect();

        let header = if start > 0 || end < total {
            format!("[Lines {}-{} of {}]", start + 1, end, total)
        } else {
            format!("[{} lines]", total)
        };

        let output = format!("{}\n{}", header, numbered.join("\n"));

        Ok(ToolResult {
            content: vec![Content::Text { text: output }],
            details: serde_json::json!({ "path": path }),
        })
    }
}

// ---------------------------------------------------------------------------

/// Write content to a file. Creates parent directories if needed.
pub struct WriteFileTool;

impl Default for WriteFileTool {
    fn default() -> Self {
        Self::new()
    }
}

impl WriteFileTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl AgentTool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn label(&self) -> &str {
        "Write File"
    }

    fn description(&self) -> &str {
        "Write contents to a file on the local filesystem.\n\
         \n\
         Usage:\n\
         - This tool will overwrite the existing file if there is one at the provided path.\n\
         - If this is an existing file, you MUST use read_file first to read the file's contents. \
         This tool will fail if you did not read the file first.\n\
         - Prefer edit_file for modifying existing files — it only sends the diff. \
         Only use this tool to create new files or for complete rewrites.\n\
         - Creates parent directories automatically if they don't exist."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path to write"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                }
            },
            "required": ["path", "content"]
        })
    }

    fn preview_command(&self, params: &serde_json::Value) -> Option<String> {
        let path = params["path"].as_str()?;
        Some(format!("cat > {}", path))
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let path = params["path"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs("missing 'path' parameter".into()))?;
        let content = params["content"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidArgs("missing 'content' parameter".into()))?;

        if ctx.cancel.is_cancelled() {
            return Err(ToolError::Cancelled);
        }

        // Read old content before writing (for diff display)
        let old_content = tokio::fs::read_to_string(path).await.ok();

        // Create parent directories
        if let Some(parent) = std::path::Path::new(path).parent() {
            if !parent.exists() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| ToolError::Failed(format!("Cannot create directory: {}", e)))?;
            }
        }

        tokio::fs::write(path, content)
            .await
            .map_err(|e| ToolError::Failed(format!("Cannot write {}: {}", path, e)))?;

        let bytes = content.len();
        let mut details =
            serde_json::json!({ "path": path, "bytes": bytes, "new_content": content });
        if let Some(old) = &old_content {
            details["old_content"] = serde_json::Value::String(old.clone());
        }
        Ok(ToolResult {
            content: vec![Content::Text {
                text: format!("Wrote {} bytes to {}", bytes, path),
            }],
            details,
        })
    }
}
