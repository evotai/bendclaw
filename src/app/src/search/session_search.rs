use super::TextMatcher;
use crate::types::SessionMeta;
use crate::types::TranscriptEntry;
use crate::types::TranscriptItem;

#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchHit {
    pub session: SessionMeta,
    pub matched_field: String,
    pub snippet: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionWithText {
    #[serde(flatten)]
    pub session: SessionMeta,
    pub search_text: String,
}

pub struct SessionSearcher {
    matcher: TextMatcher,
}

impl SessionSearcher {
    pub fn new(query: &str) -> Self {
        Self {
            matcher: TextMatcher::new(query.trim()),
        }
    }

    pub fn matches_meta(&self, session: &SessionMeta) -> Option<SearchHit> {
        if self.matcher.is_empty() {
            return Some(hit(session, "all", ""));
        }

        let fields = [
            ("title", session.title.as_deref().unwrap_or("")),
            ("cwd", &session.cwd),
            ("source", &session.source),
            ("model", &session.model),
            ("session_id", &session.session_id),
        ];

        for (name, value) in &fields {
            if self.matcher.is_substring(value) {
                return Some(hit(session, name, value));
            }
        }
        None
    }

    pub fn matches_transcript(
        &self,
        session: &SessionMeta,
        entries: &[TranscriptEntry],
    ) -> Option<SearchHit> {
        if self.matcher.is_empty() {
            return None;
        }

        for entry in entries {
            if let Some(text) = extract_text(&entry.item) {
                if self.matcher.matches(text) {
                    let snippet = truncate(text, 120);
                    return Some(hit(session, "content", &snippet));
                }
            }
        }
        None
    }
}

pub fn collect_search_text(session: &SessionMeta, entries: &[TranscriptEntry]) -> String {
    let mut parts = Vec::new();
    parts.push(session.session_id.clone());
    if let Some(t) = &session.title {
        parts.push(t.clone());
    }
    parts.push(session.cwd.clone());
    parts.push(session.source.clone());
    parts.push(session.model.clone());
    for entry in entries {
        if let Some(text) = extract_text(&entry.item) {
            parts.push(truncate(text, 200));
        }
    }
    parts.join(" ")
}

fn hit(session: &SessionMeta, field: &str, snippet: &str) -> SearchHit {
    SearchHit {
        session: session.clone(),
        matched_field: field.to_string(),
        snippet: snippet.to_string(),
    }
}

fn extract_text(item: &TranscriptItem) -> Option<&str> {
    match item {
        TranscriptItem::User { text, .. } => Some(text),
        TranscriptItem::Assistant { text, .. } => Some(text),
        TranscriptItem::ToolResult { content, .. } => Some(content),
        TranscriptItem::System { text } => Some(text),
        _ => None,
    }
}

fn truncate(s: &str, max: usize) -> String {
    let first_line = s.lines().next().unwrap_or(s);
    if first_line.chars().count() <= max {
        first_line.to_string()
    } else {
        let end: usize = first_line
            .char_indices()
            .nth(max)
            .map(|(i, _)| i)
            .unwrap_or(first_line.len());
        format!("{}…", &first_line[..end])
    }
}
