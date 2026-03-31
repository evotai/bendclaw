/// Tool description for the LLM — the first source of behavioral guidance for web_search.
pub const DESCRIPTION: &str = "\
Search the web for current information, news, documentation, or any topic. \
Returns relevant results with titles, URLs, and descriptions.\n\
\n\
Usage:\n\
- Always search first — do not construct URLs from memory.\n\
- Be specific with queries for better results.\n\
- Only use web_fetch when you need full page content from a URL found in search results.\n\
- Domain filtering is supported to include or block specific websites.\n\
- Use the correct year in search queries when searching for recent information, \
documentation, or current events. Do not default to last year.\n\
\n\
CRITICAL REQUIREMENT:\n\
- After answering the user's question using search results, you MUST include a \
\"Sources:\" section at the end of your response.\n\
- In the Sources section, list all relevant URLs from the search results as \
markdown hyperlinks: [Title](URL).\n\
- This is MANDATORY — never skip including sources in your response.";

/// Parameter descriptions.
pub const PARAM_QUERY: &str = "The search query to use. Be specific for better results.";
pub const PARAM_COUNT: &str = "Number of results to return (default: 5, max: 20).";
