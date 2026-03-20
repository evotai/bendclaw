use std::io::Cursor;

/// Extract readable content from HTML and convert it to markdown.
///
/// Returns `None` if the input cannot be parsed or has no extractable content,
/// allowing the caller to fall back to the raw text.
pub fn html_to_markdown(html: &str) -> Option<String> {
    let mut cursor = Cursor::new(html.as_bytes());
    let article = readability::extractor::extract(&mut cursor, &reqwest::Url::parse("https://example.com").ok()?).ok()?;

    let md = htmd::convert(&article.content).ok()?;
    let trimmed = md.trim();
    if trimmed.is_empty() {
        return None;
    }

    let title = article.title.trim();
    if title.is_empty() {
        Some(trimmed.to_string())
    } else {
        Some(format!("# {title}\n\n{trimmed}"))
    }
}
