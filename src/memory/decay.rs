//! Memory decay — access tracking and relevance scoring.

/// Compute a decay score for a memory entry.
///
/// Higher score = more relevant. Based on access frequency and recency.
/// - `days_since_access`: days since last access
/// - `access_count`: total access count
/// - `half_life_days`: half-life for exponential decay (default: 30)
///
/// Returns a score in (0.0, 1.0].
pub fn decay_score(days_since_access: f64, access_count: u32, half_life_days: f64) -> f64 {
    let half_life = if half_life_days <= 0.0 {
        30.0
    } else {
        half_life_days
    };

    // Exponential time decay
    let time_factor = (-(days_since_access * 0.693) / half_life).exp();

    // Access frequency boost: log2(1 + count) / 5, capped at 1.0
    let freq_boost = ((1.0 + access_count as f64).log2() / 5.0).min(1.0);

    // Blend: 60% time, 40% frequency
    let score = 0.6 * time_factor + 0.4 * freq_boost;

    score.clamp(0.001, 1.0)
}
