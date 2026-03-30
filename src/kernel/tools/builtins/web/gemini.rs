use serde::Deserialize;

const DEFAULT_MODEL: &str = "gemini-2.5-flash";

/// Query Gemini with Google Search grounding. Returns formatted output string.
pub async fn search(
    client: &reqwest::Client,
    base_url: &str,
    query: &str,
    count: u32,
    api_key: &str,
) -> Result<String, String> {
    let url = format!("{base_url}/models/{DEFAULT_MODEL}:generateContent");

    let body = serde_json::json!({
        "contents": [{ "parts": [{ "text": query }] }],
        "tools": [{ "google_search": {} }]
    });

    let resp = client
        .post(&url)
        .header("x-goog-api-key", api_key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Gemini request failed: {e}"))?;

    let status = resp.status();
    let raw = resp
        .text()
        .await
        .map_err(|e| format!("Gemini body read failed: {e}"))?;

    if !status.is_success() {
        return Err(format!("Gemini API HTTP {status}"));
    }

    let data: GeminiResponse =
        serde_json::from_str(&raw).map_err(|e| format!("Failed to parse Gemini response: {e}"))?;

    let candidate = match data.candidates.as_deref() {
        Some([first, ..]) => first,
        _ => return Ok("No response".to_string()),
    };

    Ok(format_candidate(candidate, count))
}

/// Format a single candidate into the output string.
fn format_candidate(candidate: &Candidate, count: u32) -> String {
    let content: String = candidate
        .content
        .as_ref()
        .and_then(|c| c.parts.as_ref())
        .map(|parts| {
            parts
                .iter()
                .filter_map(|p| p.text.as_deref())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_else(|| "No response".to_string());

    let chunks = candidate
        .grounding_metadata
        .as_ref()
        .and_then(|m| m.grounding_chunks.as_deref())
        .unwrap_or_default();

    let citations: Vec<_> = chunks
        .iter()
        .filter_map(|chunk| {
            let web = chunk.web.as_ref()?;
            Some((
                web.title.as_deref().unwrap_or_default(),
                web.uri.as_deref()?,
            ))
        })
        .take(count as usize)
        .collect();

    if citations.is_empty() {
        return content;
    }

    let lines: Vec<String> = citations
        .iter()
        .enumerate()
        .map(|(i, (title, url))| format!("{}. {title}\n{url}", i + 1))
        .collect();
    format!(
        "Content:\n{content}\n\nCitations:\n\n{}",
        lines.join("\n\n")
    )
}

// --- Gemini API response types ---

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Option<Vec<Candidate>>,
}

#[derive(Deserialize)]
struct Candidate {
    content: Option<Content>,
    #[serde(rename = "groundingMetadata")]
    grounding_metadata: Option<GroundingMetadata>,
}

#[derive(Deserialize)]
struct Content {
    parts: Option<Vec<Part>>,
}

#[derive(Deserialize)]
struct Part {
    text: Option<String>,
}

#[derive(Deserialize)]
struct GroundingMetadata {
    #[serde(rename = "groundingChunks")]
    grounding_chunks: Option<Vec<GroundingChunk>>,
}

#[derive(Deserialize)]
struct GroundingChunk {
    web: Option<WebChunk>,
}

#[derive(Deserialize)]
struct WebChunk {
    uri: Option<String>,
    title: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_and_format(json_str: &str, count: u32) -> String {
        let data: GeminiResponse = serde_json::from_str(json_str).expect("parse");
        format_candidate(&data.candidates.expect("candidates")[0], count)
    }

    const RESPONSE_WITH_CHUNKS: &str = r#"{
        "candidates": [{
            "content": { "parts": [{ "text": "Spain won Euro 2024." }] },
            "groundingMetadata": {
                "groundingChunks": [
                    { "web": { "uri": "https://wiki.org/euro", "title": "Wikipedia" } },
                    { "web": { "uri": "https://example.com", "title": "Example" } },
                    { "web": { "uri": "https://third.com", "title": "Third" } }
                ]
            }
        }]
    }"#;

    #[test]
    fn text_only_without_citations() {
        assert_eq!(
            parse_and_format(
                r#"{ "candidates": [{ "content": { "parts": [{ "text": "Hello world." }] } }] }"#,
                10,
            ),
            "Hello world."
        );
    }

    #[test]
    fn content_with_citations() {
        assert_eq!(
            parse_and_format(RESPONSE_WITH_CHUNKS, 10),
            "Content:\nSpain won Euro 2024.\n\nCitations:\n\n1. Wikipedia\nhttps://wiki.org/euro\n\n2. Example\nhttps://example.com\n\n3. Third\nhttps://third.com"
        );
    }

    #[test]
    fn citations_respect_count_limit() {
        let output = parse_and_format(RESPONSE_WITH_CHUNKS, 2);
        assert!(output.contains("1. Wikipedia"));
        assert!(output.contains("2. Example"));
        assert!(!output.contains("Third"));
    }

    #[test]
    fn no_candidates_returns_no_response() {
        let data: GeminiResponse = serde_json::from_str(r#"{ "candidates": [] }"#).expect("parse");
        assert!(data.candidates.as_deref().is_some_and(|c| c.is_empty()));
    }

    #[test]
    fn multi_part_text_joined_with_newline() {
        assert_eq!(
            parse_and_format(
                r#"{ "candidates": [{ "content": { "parts": [{ "text": "Line 1" }, { "text": "Line 2" }] } }] }"#,
                10,
            ),
            "Line 1\nLine 2"
        );
    }
}
