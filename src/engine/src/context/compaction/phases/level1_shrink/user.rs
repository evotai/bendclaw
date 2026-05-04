use crate::context::tokens::content_tokens;
use crate::types::Content;

// ---------------------------------------------------------------------------
// User message classification
// ---------------------------------------------------------------------------

/// What to do with a user message during shrink.
pub(super) enum UserAction {
    /// Over budget + oversized: truncate text
    TruncateOversized,
    /// Over budget + has images: strip images
    StripImages,
    /// No action needed
    Keep,
}

/// Classify the action for a user message based on position and size.
pub(super) fn classify_user_action(
    is_pinned: bool,
    is_recent: bool,
    running_tokens: usize,
    budget: usize,
    content: &[Content],
    oversize_threshold: usize,
) -> UserAction {
    let before_tokens = content_tokens(content);
    let has_text = content.iter().any(|c| matches!(c, Content::Text { .. }));
    if !is_pinned
        && !is_recent
        && running_tokens > budget
        && before_tokens > oversize_threshold
        && has_text
    {
        return UserAction::TruncateOversized;
    }

    let severely_over = running_tokens > budget + budget / 2;
    if severely_over
        && running_tokens > budget
        && content.iter().any(|c| matches!(c, Content::Image { .. }))
    {
        return UserAction::StripImages;
    }

    UserAction::Keep
}
