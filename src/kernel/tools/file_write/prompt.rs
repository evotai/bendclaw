/// Tool description for the LLM — the first source of behavioral guidance for file_write.
pub const DESCRIPTION: &str = "\
Write contents to a file on the local filesystem.\n\
\n\
Usage:\n\
- This tool will overwrite the existing file if there is one at the provided path.\n\
- If this is an existing file, you MUST use file_read first to read the file's contents. \
This tool will fail if you did not read the file first.\n\
- Prefer file_edit for modifying existing files — it only sends the diff. \
Only use this tool to create new files or for complete rewrites.\n\
- NEVER create documentation files (*.md) or README files unless explicitly requested by the User.\n\
- Only use emojis if the user explicitly requests it. Avoid writing emojis to files unless asked.\n\
- Accepts absolute or workspace-relative paths. Creates parent directories as needed.";

/// Parameter descriptions.
pub const PARAM_PATH: &str = "Absolute or workspace-relative path to the file to write.";
pub const PARAM_CONTENT: &str = "Content to write to the file.";
