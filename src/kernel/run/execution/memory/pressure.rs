//! Context pressure assessment — pure functions, no dependencies.

/// How close the context window is to capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressureLevel {
    /// < 70% — no action needed.
    Normal,
    /// 70–85% — extract memories only, don't compact yet.
    Elevated,
    /// 85–100% — extract memories, then compact.
    High,
    /// > 100% — skip extraction, compact immediately.
    Critical,
}

/// Assess context pressure from token counts.
pub fn assess(total_tokens: usize, max_tokens: usize) -> PressureLevel {
    if max_tokens == 0 {
        return PressureLevel::Critical;
    }
    // Use integer math: ratio = total * 100 / max
    let pct = total_tokens.saturating_mul(100) / max_tokens;
    match pct {
        0..=69 => PressureLevel::Normal,
        70..=84 => PressureLevel::Elevated,
        85..=100 => PressureLevel::High,
        _ => PressureLevel::Critical,
    }
}
