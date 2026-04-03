use std::time::Duration;

/// A single DuckDuckGo search result.
pub struct DdgResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Query DuckDuckGo HTML endpoint and parse results.
pub async fn search(
    client: &reqwest::Client,
    query: &str,
    count: u32,
) -> Result<Vec<DdgResult>, String> {
    let resp = client
        .get("https://html.duckduckgo.com/html/")
        .timeout(Duration::from_secs(15))
        .query(&[("q", query)])
        .send()
        .await
        .map_err(|e| format!("DuckDuckGo request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("DuckDuckGo HTTP {}", resp.status()));
    }

    let body = resp
        .text()
        .await
        .map_err(|e| format!("DuckDuckGo body read failed: {e}"))?;

    let results = parse_results(&body, count as usize);
    Ok(results)
}

/// Format DDG results into the same text layout as Brave results.
pub fn format_results(results: &[DdgResult]) -> String {
    if results.is_empty() {
        return "No results found.".to_string();
    }
    let items: Vec<String> = results
        .iter()
        .enumerate()
        .map(|(i, r)| format!("{}. {}\n{}\n{}", i + 1, r.title, r.url, r.snippet))
        .collect();
    format!("Found {} results:\n\n{}", results.len(), items.join("\n\n"))
}

/// Expose `parse_results` for integration tests.
pub fn parse_results_for_test(html: &str, max: usize) -> Vec<DdgResult> {
    parse_results(html, max)
}

/// Parse DuckDuckGo HTML response into structured results.
///
/// DDG HTML results look like:
/// ```html
/// <a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com&...">Title</a>
/// <a class="result__snippet" href="...">Snippet text</a>
/// ```
fn parse_results(html: &str, max: usize) -> Vec<DdgResult> {
    let mut results = Vec::new();

    // Split on result__a anchors to find each result block
    let parts: Vec<&str> = html.split("class=\"result__a\"").collect();

    // First part is before any result, skip it
    for part in parts.iter().skip(1) {
        if results.len() >= max {
            break;
        }

        let title = extract_between(part, ">", "</a>").unwrap_or_default();
        let title = strip_html_tags(&title);

        let raw_href = extract_between(part, "href=\"", "\"").unwrap_or_default();
        let url = decode_ddg_href(&raw_href);

        let snippet = if let Some(snip_part) = part.split("class=\"result__snippet\"").nth(1) {
            let s = extract_between(snip_part, ">", "</a>").unwrap_or_default();
            strip_html_tags(&s)
        } else {
            String::new()
        };

        if !url.is_empty() {
            results.push(DdgResult {
                title,
                url,
                snippet,
            });
        }
    }

    results
}

/// Extract text between two delimiters.
fn extract_between(s: &str, start: &str, end: &str) -> Option<String> {
    let start_idx = s.find(start)? + start.len();
    let rest = &s[start_idx..];
    let end_idx = rest.find(end)?;
    Some(rest[..end_idx].to_string())
}

/// Decode a DDG redirect href into the actual target URL.
///
/// DDG wraps links as `//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com&rut=...`
fn decode_ddg_href(href: &str) -> String {
    // Look for uddg= parameter
    if let Some(pos) = href.find("uddg=") {
        let encoded = &href[pos + 5..];
        // Take until next & or end
        let encoded = encoded.split('&').next().unwrap_or(encoded);
        url_decode(encoded)
    } else if href.starts_with("http") {
        href.to_string()
    } else if href.starts_with("//") {
        format!("https:{href}")
    } else {
        href.to_string()
    }
}

/// Minimal percent-decoding for URL strings.
fn url_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let hi = chars.next().unwrap_or(b'0');
            let lo = chars.next().unwrap_or(b'0');
            let byte = hex_val(hi) << 4 | hex_val(lo);
            result.push(byte as char);
        } else if b == b'+' {
            result.push(' ');
        } else {
            result.push(b as char);
        }
    }
    result
}

fn hex_val(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}

/// Strip HTML tags from a string, keeping only text content.
fn strip_html_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    // Collapse whitespace
    let mut result = String::with_capacity(out.len());
    let mut prev_space = false;
    for ch in out.chars() {
        if ch.is_whitespace() {
            if !prev_space {
                result.push(' ');
            }
            prev_space = true;
        } else {
            result.push(ch);
            prev_space = false;
        }
    }
    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ddg_html_extracts_results() {
        let html = r#"
        <div class="result">
            <a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fpage&rut=abc">Example <b>Title</b></a>
            <a class="result__snippet" href="...">This is the <b>snippet</b> text.</a>
        </div>
        <div class="result">
            <a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fother.com&rut=def">Other Site</a>
            <a class="result__snippet" href="...">Another snippet.</a>
        </div>
        "#;

        let results = parse_results(html, 10);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Example Title");
        assert_eq!(results[0].url, "https://example.com/page");
        assert_eq!(results[0].snippet, "This is the snippet text.");
        assert_eq!(results[1].url, "https://other.com");
    }

    #[test]
    fn parse_ddg_html_respects_max() {
        let html = r#"
            <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fa.com">A</a>
            <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fb.com">B</a>
            <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fc.com">C</a>
        "#;
        let results = parse_results(html, 2);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn url_decode_handles_encoded_url() {
        assert_eq!(
            url_decode("https%3A%2F%2Fexample.com%2Fpath%3Fq%3Dhello+world"),
            "https://example.com/path?q=hello world"
        );
    }

    #[test]
    fn decode_ddg_href_extracts_uddg() {
        let href = "//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com&rut=abc123";
        assert_eq!(decode_ddg_href(href), "https://example.com");
    }

    #[test]
    fn decode_ddg_href_passthrough_direct_url() {
        assert_eq!(
            decode_ddg_href("https://example.com"),
            "https://example.com"
        );
    }
}
