/// Tool description for the LLM — the first source of behavioral guidance for file_edit.
pub const DESCRIPTION: &str = "\
Perform exact string replacements in files.\n\
\n\
Usage:\n\
- You must use file_read at least once in the conversation before editing. \
This tool will error if you attempt an edit without reading the file first.\n\
- When editing text from file_read output, ensure you preserve the exact indentation \
(tabs/spaces) as it appears AFTER the line number prefix. The line number prefix format is: \
line number + tab. Everything after that is the actual file content to match. \
Never include any part of the line number prefix in old_string or new_string.\n\
- ALWAYS prefer editing existing files in the codebase. NEVER write new files unless \
explicitly required.\n\
- Only use emojis if the user explicitly requests it. Avoid adding emojis to files \
unless asked.\n\
- The edit will FAIL if old_string is not unique in the file. Either provide a larger \
string with more surrounding context to make it unique, or use replace_all to change \
every instance of old_string.\n\
- Use replace_all for replacing and renaming strings across the file. This is useful \
if you want to rename a variable for instance.\n\
- Use this tool instead of shell sed/awk for file modifications.";

/// Parameter descriptions.
pub const PARAM_PATH: &str = "Absolute or workspace-relative path to the file to edit.";
pub const PARAM_OLD_STRING: &str = "The exact string to search for in the file. Must match exactly once unless replace_all is set.";
pub const PARAM_NEW_STRING: &str = "The replacement string.";
