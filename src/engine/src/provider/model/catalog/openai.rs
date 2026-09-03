use super::super::capabilities::AnthropicThinkingWire;
use super::super::capabilities::Verbosity;
use super::profile::ModelProfile;
use super::profile::ReasoningProfile;
use super::profile::BASE;
use crate::ThinkingLevel;

const GPT_LEVELS: &[(ThinkingLevel, Option<&str>)] = &[
    (ThinkingLevel::Low, Some("low")),
    (ThinkingLevel::Medium, Some("medium")),
    (ThinkingLevel::High, Some("high")),
    (ThinkingLevel::Xhigh, Some("xhigh")),
];
const GPT_5_6_LEVELS: &[(ThinkingLevel, Option<&str>)] = &[
    (ThinkingLevel::Off, Some("none")),
    (ThinkingLevel::Low, Some("low")),
    (ThinkingLevel::Medium, Some("medium")),
    (ThinkingLevel::High, Some("high")),
    (ThinkingLevel::Xhigh, Some("xhigh")),
    (ThinkingLevel::Max, Some("max")),
];
const GPT_5_5_PRO_LEVELS: &[(ThinkingLevel, Option<&str>)] = &[
    (ThinkingLevel::Medium, Some("medium")),
    (ThinkingLevel::High, Some("high")),
    (ThinkingLevel::Xhigh, Some("xhigh")),
];
const LEGACY_LEVELS: &[(ThinkingLevel, Option<&str>)] = &[
    (ThinkingLevel::Off, Some("none")),
    (ThinkingLevel::Low, Some("low")),
    (ThinkingLevel::Medium, Some("medium")),
    (ThinkingLevel::High, Some("high")),
];

const GPT_REASONING: ReasoningProfile = ReasoningProfile {
    levels: GPT_LEVELS,
    default: ThinkingLevel::Medium,
    anthropic_wire: Some(AnthropicThinkingWire::Enabled),
};
const GPT_5_6_REASONING: ReasoningProfile = ReasoningProfile {
    levels: GPT_5_6_LEVELS,
    default: ThinkingLevel::Medium,
    anthropic_wire: Some(AnthropicThinkingWire::Enabled),
};
const GPT_5_5_PRO_REASONING: ReasoningProfile = ReasoningProfile {
    levels: GPT_5_5_PRO_LEVELS,
    default: ThinkingLevel::Medium,
    anthropic_wire: Some(AnthropicThinkingWire::Enabled),
};
const LEGACY_REASONING: ReasoningProfile = ReasoningProfile {
    levels: LEGACY_LEVELS,
    default: ThinkingLevel::Medium,
    anthropic_wire: Some(AnthropicThinkingWire::Enabled),
};

// 1M total context window; the input limit below is the window minus output
// headroom, which is what users recognize as the model's window size.
const GPT_5_6: ModelProfile = ModelProfile {
    max_input_tokens: 922_000,
    advertised_context_window: Some(1_000_000),
    max_output_tokens: 128_000,
    reasoning: GPT_5_6_REASONING,
    remote_compaction: true,
    default_verbosity: Some(Verbosity::Low),
    ..BASE
};

#[rustfmt::skip]
const PROFILES: &[(&str, ModelProfile)] = &[
    ("gpt-5.4",       ModelProfile { max_input_tokens: 922_000, advertised_context_window: Some(1_000_000), max_output_tokens: 128_000, reasoning: GPT_REASONING, remote_compaction: true, ..BASE }),
    ("gpt-5.4-pro",   ModelProfile { max_input_tokens: 922_000, advertised_context_window: Some(1_000_000), max_output_tokens: 128_000, reasoning: GPT_REASONING, remote_compaction: true, ..BASE }),
    ("gpt-5.5",       ModelProfile { max_input_tokens: 922_000, advertised_context_window: Some(1_000_000), max_output_tokens: 128_000, reasoning: GPT_REASONING, remote_compaction: true, default_verbosity: Some(Verbosity::Low), ..BASE }),
    ("gpt-5.5-pro",   ModelProfile { max_input_tokens: 922_000, advertised_context_window: Some(1_000_000), max_output_tokens: 128_000, reasoning: GPT_5_5_PRO_REASONING, remote_compaction: true, ..BASE }),
    ("gpt-5.6-luna",  GPT_5_6),
    ("gpt-5.6-sol",   GPT_5_6),
    ("gpt-5.6-terra", GPT_5_6),
    ("gpt-6-astra",   GPT_5_6),
];

pub(super) fn resolve(id: &str) -> Option<ModelProfile> {
    PROFILES
        .iter()
        .find_map(|(candidate, profile)| (*candidate == id).then_some(*profile))
}

/// Conservative metadata for uncatalogued OpenAI reasoning families.
/// GPT 5.4+ inherits the current 1M window; older ids stay on 128k.
pub(super) fn fallback(id: &str) -> Option<ModelProfile> {
    if id.starts_with("gpt-") || id.starts_with("codex-") {
        let reasoning = if id.contains("gpt-5.6") {
            GPT_5_6_REASONING
        } else if ["gpt-5.4", "gpt-5.5"]
            .iter()
            .any(|family| id.contains(family))
        {
            GPT_REASONING
        } else {
            LEGACY_REASONING
        };
        let profile = ModelProfile {
            max_input_tokens: 128_000,
            max_output_tokens: 32_768,
            reasoning,
            ..BASE
        };
        return Some(if super::profile::version_at_least(id, "gpt-", (5, 4)) {
            profile.with_window(GPT_5_6)
        } else {
            profile
        });
    }
    if id.starts_with("o1") || id.starts_with("o3") || id.starts_with("o4") {
        return Some(ModelProfile {
            max_input_tokens: 128_000,
            max_output_tokens: 32_768,
            vision: false,
            reasoning: LEGACY_REASONING,
            ..BASE
        });
    }
    None
}
