/// Tool description for the LLM — the first source of behavioral guidance for web_fetch.
pub const DESCRIPTION: &str = "\
Fetch a URL and return its content. HTML pages are converted to readable markdown. \
Also works for JSON/text APIs.\n\
\n\
Usage:\n\
- Use this tool when you need to retrieve and analyze web content.\n\
- Use after web_search to read a specific URL — do not guess URLs from memory.\n\
- The URL must be a fully-formed valid URL.\n\
- HTTP URLs will be automatically upgraded to HTTPS.\n\
- This tool is read-only and does not modify any files.\n\
- Results may be summarized if the content is very large.\n\
- Includes a self-cleaning 15-minute cache for faster responses when repeatedly \
accessing the same URL.\n\
- When a URL redirects to a different host, the tool will inform you and provide \
the redirect URL. You should then make a new request with the redirect URL.\n\
- For GitHub URLs, prefer using the gh CLI via shell instead \
(e.g., gh pr view, gh issue view, gh api).\n\
- You MUST include the relevant data in your response — quote specific facts, \
numbers, or passages.";

/// Parameter descriptions.
pub const PARAM_URL: &str = "The URL to fetch. Must be a fully-formed valid URL.";
