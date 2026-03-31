/// Tool description for the LLM — the first source of behavioral guidance for shell.
///
/// This is the most constraint-heavy prompt. It establishes tool preference hierarchy,
/// command patterns, git safety, and sleep avoidance — all aligned with claudecode's
/// BashTool prompt.
pub const DESCRIPTION: &str = "\
Execute a shell command and return its output.\n\
\n\
The working directory persists between commands, but shell state does not.\n\
\n\
IMPORTANT: Avoid using this tool to run grep, find, cat, head, tail, sed, awk, or echo \
commands, unless explicitly instructed or after you have verified that a dedicated tool \
cannot accomplish your task. Instead, use the appropriate dedicated tool:\n\
\n\
- File search: Use glob (NOT find)\n\
- Directory listing: Use list_dir (NOT ls)\n\
- Content search: Use grep tool (NOT shell grep or rg)\n\
- Read files: Use file_read (NOT cat/head/tail)\n\
- Edit files: Use file_edit (NOT sed/awk)\n\
- Write files: Use file_write (NOT echo/cat redirection)\n\
\n\
The built-in tools provide a better experience and make it easier to review operations.\n\
\n\
# Instructions\n\
- If your command will create new directories or files, first use list_dir to verify the \
parent directory exists and is the correct location.\n\
- Always quote file paths that contain spaces with double quotes.\n\
- Try to maintain your current working directory by using absolute paths and avoiding cd.\n\
- When issuing multiple commands:\n\
  - If independent and can run in parallel, make multiple tool calls in a single message.\n\
  - If dependent and must run sequentially, chain with && in a single call.\n\
  - Use ; only when you need sequential execution but don't care if earlier commands fail.\n\
  - DO NOT use newlines to separate commands.\n\
- For git commands:\n\
  - Prefer creating a new commit rather than amending an existing commit.\n\
  - Before running destructive operations (git reset --hard, git push --force, \
git checkout --), consider safer alternatives.\n\
  - Never skip hooks (--no-verify) or bypass signing unless the user explicitly asks.\n\
- Avoid unnecessary sleep commands:\n\
  - Do not sleep between commands that can run immediately.\n\
  - Do not retry failing commands in a sleep loop — diagnose the root cause.";

/// Parameter descriptions.
pub const PARAM_COMMAND: &str = "The shell command to execute.";
