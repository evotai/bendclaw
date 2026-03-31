/// Tool description for the LLM — the first source of behavioral guidance for file_read.
pub const DESCRIPTION: &str = "\
Read a text file from the local filesystem. You can access any file directly by using this tool.\n\
Assume this tool is able to read all files on the machine. If the User provides a path \
to a file assume that path is valid. It is okay to read a file that does not exist; \
an error will be returned.\n\
\n\
Usage:\n\
- The path parameter must be an absolute path, not a relative path.\n\
- Use this tool instead of shell cat/head/tail for reading files.\n\
- This tool reads the entire file at once.\n\
- This tool can only read text files, not directories or binary files (images, PDFs, etc.). \
To list a directory, use list_dir.\n\
- If you read a file that exists but has empty contents you will receive a warning \
in place of file contents.";

/// Parameter descriptions.
pub const PARAM_PATH: &str = "Absolute or workspace-relative path to the file to read.";
