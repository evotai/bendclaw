/// Tool description for the LLM — the first source of behavioral guidance for glob.
pub const DESCRIPTION: &str = "\
Fast file pattern matching tool that works with any codebase size.\n\
\n\
Usage:\n\
- ALWAYS use this tool to find files. NEVER use shell with find or ls for file discovery.\n\
- Supports glob patterns like \"**/*.rs\" or \"src/**/*.ts\".\n\
- Returns matching file paths sorted by modification time.\n\
- Respects .gitignore.\n\
- Use this tool when you need to find files by name patterns.\n\
- For open-ended searches requiring multiple rounds of globbing and grepping, \
break the search into smaller targeted queries.";

/// Parameter descriptions.
pub const PARAM_PATTERN: &str =
    "Glob pattern to match file names, e.g. '*.rs', '*.test.ts', 'Cargo.toml'.";
pub const PARAM_PATH: &str =
    "Absolute or relative path to search in. Defaults to the workspace directory.";
