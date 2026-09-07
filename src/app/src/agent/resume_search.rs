//! Semantic session search for `/resume <query>`.
//!
//! Ranks past sessions against a free-form query with a one-shot LLM call, so
//! recall works on meaning (synonyms, translations, related concepts) rather
//! than literal substrings. The gateway exposes it as the hidden `/_rsearch`
//! command; the CLI routes non-id `/resume <query>` arguments through it.

use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::conf::LlmConfig;
use crate::error::EvotError;
use crate::error::Result;
use crate::search::SessionWithText;

/// Most recent sessions considered per search.
pub const SESSION_LIMIT: usize = 30;
/// Characters of per-session transcript text included in the ranking prompt.
const TEXT_BUDGET: usize = 1200;
/// Cap on ranked results returned to the user.
const MAX_RESULTS: usize = 10;
/// Wall-clock budget for the ranking LLM call.
const RANK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(45);

const SYSTEM_PROMPT: &str =
    "You rank coding-agent sessions by semantic relevance to a user query. \
Match on meaning, not literal keywords: consider synonyms, related concepts, and translated terms. \
Reply with one line per relevant session, most relevant first: <session_id> | <one-line reason>. \
Only include sessions that are genuinely relevant to the query. \
If none are relevant, reply with exactly NONE. Output nothing else.";

/// Provider + model configuration for the ranking call.
pub struct RankContext {
    pub provider: Arc<dyn evot_engine::provider::StreamProvider>,
    pub llm: LlmConfig,
}

/// Rank `sessions` against `query` and return a human-readable result list.
pub async fn rank_sessions(
    ctx: &RankContext,
    query: &str,
    sessions: &[SessionWithText],
) -> Result<String> {
    if sessions.is_empty() {
        return Ok("No sessions to search.".to_string());
    }
    let query = normalize_query(query);
    let prompt = build_rank_prompt(query, sessions);
    let response = tokio::time::timeout(RANK_TIMEOUT, call_provider(ctx, &prompt))
        .await
        .map_err(|_| EvotError::Run("session search timed out".to_string()))??;
    Ok(format_results(query, &response, sessions))
}

/// Build the ranking prompt: query first, then one block per session with its
/// id, last-update time, and a bounded transcript excerpt.
pub fn build_rank_prompt(query: &str, sessions: &[SessionWithText]) -> String {
    let mut out = format!("Query: {query}\n\nSessions:\n");
    for s in sessions.iter().take(SESSION_LIMIT) {
        out.push_str(&format!(
            "--- id: {}\nupdated: {}\n{}\n",
            s.session.session_id,
            s.session.updated_at,
            truncate_chars(&s.search_text, TEXT_BUDGET),
        ));
    }
    out
}

/// Turn the LLM response (`<session_id> | <reason>` lines or `NONE`) into the
/// user-facing list. Lines whose id does not match a real session are dropped,
/// so hallucinated ids never surface.
pub fn format_results(query: &str, response: &str, sessions: &[SessionWithText]) -> String {
    let response = response.trim();
    let mut out = String::new();
    let mut count = 0usize;

    for line in response.lines() {
        let line = line.trim().trim_start_matches('-').trim();
        let Some((id, reason)) = line.split_once('|') else {
            continue;
        };
        let (id, reason) = (id.trim(), reason.trim());
        let Some(hit) = sessions.iter().find(|s| s.session.session_id == id) else {
            continue;
        };
        let title = hit.session.display_title().unwrap_or("(untitled)");
        out.push_str(&format!("- {id} — {title} — {reason}\n"));
        count += 1;
        if count == MAX_RESULTS {
            break;
        }
    }

    if count == 0 {
        return format!("No sessions relevant to '{query}'.");
    }
    format!("Sessions relevant to '{query}':\n\n{out}\nResume with /resume <id>.")
}

pub fn literal_results(query: &str, sessions: &[SessionWithText]) -> Option<String> {
    let query = normalize_query(query);
    let matches: Vec<_> = sessions
        .iter()
        .filter(|session| contains_ignore_case(&session.search_text, query))
        .take(MAX_RESULTS)
        .collect();
    if matches.is_empty() {
        return None;
    }

    let mut out = String::new();
    for hit in matches {
        let id = &hit.session.session_id;
        let title = hit.session.display_title().unwrap_or("(untitled)");
        out.push_str(&format!("- {id} — {title} — exact text match\n"));
    }
    Some(format!(
        "Sessions relevant to '{query}':\n\n{out}\nResume with /resume <id>."
    ))
}

fn normalize_query(query: &str) -> &str {
    let query = query.trim();
    for (open, close) in [("'", "'"), ("\"", "\""), ("‘", "’"), ("“", "”")] {
        if let Some(inner) = query
            .strip_prefix(open)
            .and_then(|value| value.strip_suffix(close))
        {
            return inner.trim();
        }
    }
    query
}

fn contains_ignore_case(text: &str, query: &str) -> bool {
    let query = query.trim().to_lowercase();
    !query.is_empty() && text.to_lowercase().contains(&query)
}

fn truncate_chars(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

async fn call_provider(ctx: &RankContext, user_prompt: &str) -> Result<String> {
    use evot_engine::provider::StreamConfig;
    use evot_engine::provider::StreamEvent;

    let messages = vec![evot_engine::Message::User {
        content: vec![evot_engine::Content::Text {
            text: user_prompt.to_string(),
        }],
        timestamp: evot_engine::now_ms(),
    }];

    let config = StreamConfig {
        model: ctx.llm.model.clone(),
        system_prompt: SYSTEM_PROMPT.to_string(),
        messages,
        tools: vec![],
        thinking_level: ctx.llm.thinking_level,
        api_key: ctx.llm.api_key.clone(),
        max_tokens: Some(1024),
        model_config: Some(ctx.llm.model_config.clone()),
        cache_config: evot_engine::CacheConfig::default(),
        prompt_cache_key: None,
    };

    let (tx, mut rx) = mpsc::unbounded_channel::<StreamEvent>();
    let cancel = CancellationToken::new();
    let result = ctx
        .provider
        .stream(config, tx, cancel)
        .await
        .map_err(|e| EvotError::Run(format!("session search LLM call failed: {e}")))?;

    // Drain the channel; only the final message matters.
    while rx.recv().await.is_some() {}

    match result.into_message() {
        evot_engine::Message::Assistant {
            content,
            stop_reason,
            error_message,
            ..
        } => {
            if stop_reason == evot_engine::StopReason::Error {
                return Err(EvotError::Run(
                    error_message.unwrap_or_else(|| "unknown LLM error".to_string()),
                ));
            }
            Ok(content
                .iter()
                .filter_map(|c| match c {
                    evot_engine::Content::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"))
        }
        _ => Err(EvotError::Run("unexpected LLM response type".to_string())),
    }
}
