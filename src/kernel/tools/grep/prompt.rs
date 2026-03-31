/// Tool description for the LLM — the first source of behavioral guidance for grep.
pub const DESCRIPTION: &str = "\
A powerful search tool built on ripgrep.\n\
\n\
Usage:\n\
- ALWAYS use this tool for content search. NEVER invoke grep or rg as a shell command. \
This tool has been optimized for correct permissions and access.\n\
- Supports full regex syntax (e.g., \"log.*Error\", \"function\\\\s+\\\\w+\").\n\
- Filter files with file_pattern parameter (e.g., \"*.rs\", \"*.py\").\n\
- Respects .gitignore. Returns matching lines with file paths and line numbers.\n\
- Pattern syntax: Uses ripgrep (not grep) — literal braces need escaping \
(use `interface\\\\{\\\\}` to find `interface{}` in Go code).\n\
- For open-ended searches requiring multiple rounds, break the search into smaller queries.";

/// Parameter descriptions.
pub const PARAM_PATTERN: &str = "Regular expression pattern to search for.";
pub const PARAM_PATH: &str =
    "Absolute or relative path to search in. Defaults to the workspace directory.";
pub const PARAM_FILE_PATTERN: &str = "Optional glob to filter files (e.g. '*.rs', '*.py').";
