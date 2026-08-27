use super::profile::ModelProfile;
use super::profile::ReasoningProfile;
use super::profile::BASE;
use crate::ThinkingLevel;

// GLM-5.2 exposes two thinking-effort tiers (docs.z.ai): "high" and "max".
// The top tier is Evot's Xhigh level; ThinkingLevel::Max is reserved for models
// exposing both tiers, so binding "max" there would strand Xhigh users on High.
const GLM_5_2_LEVELS: &[(ThinkingLevel, Option<&str>)] = &[
    (ThinkingLevel::Off, Some("none")),
    (ThinkingLevel::High, Some("high")),
    (ThinkingLevel::Xhigh, Some("max")),
];
// GLM-5.2's Anthropic-compatible endpoint accepts `output_config.effort`
// alongside `thinking.type=enabled` — same dialect as Kimi.
const GLM_5_2_REASONING: ReasoningProfile = ReasoningProfile {
    levels: GLM_5_2_LEVELS,
    default: ThinkingLevel::High,
    anthropic_wire: Some(super::super::capabilities::AnthropicThinkingWire::Enabled),
};

// 1M total window (docs.z.ai) minus the 131_072 output budget.
const GLM_5_2: ModelProfile = ModelProfile {
    max_input_tokens: 917_504,
    advertised_context_window: Some(1_000_000),
    max_output_tokens: 131_072,
    vision: false,
    reasoning: GLM_5_2_REASONING,
    ..BASE
};

/// Ox Alpha — pre-release GLM-family reasoning model (OpenRouter `stealth`
/// listing). Exposes three opencode-style efforts, low / high / max, with max
/// as the shipped default. Reasoning is mandatory: there is no Off tier.
const OX_ALPHA_LEVELS: &[(ThinkingLevel, Option<&str>)] = &[
    (ThinkingLevel::Low, Some("low")),
    (ThinkingLevel::High, Some("high")),
    (ThinkingLevel::Max, Some("max")),
];
const OX_ALPHA_REASONING: ReasoningProfile = ReasoningProfile {
    levels: OX_ALPHA_LEVELS,
    default: ThinkingLevel::Max,
    anthropic_wire: Some(super::super::capabilities::AnthropicThinkingWire::Enabled),
};

// Same window class as GLM-5.2: 1Mi total minus the 131_072 output budget;
// multimodal input (text + image).
const OX_ALPHA: ModelProfile = ModelProfile {
    max_input_tokens: 917_504,
    advertised_context_window: Some(1_048_576),
    max_output_tokens: 131_072,
    vision: true,
    reasoning: OX_ALPHA_REASONING,
    ..BASE
};

#[rustfmt::skip]
const PROFILES: &[(&str, ModelProfile)] = &[
    ("glm-5.2",      GLM_5_2),
    ("glm-5.2-fast", GLM_5_2),
    ("glm-5p2",      GLM_5_2),
    // OpenRouter serves the model as "stealth/ox-alpha"; the bare id covers
    // direct Z.ai endpoints and vendor-prefixed specs ("zai/ox-alpha").
    ("ox-alpha",         OX_ALPHA),
    ("stealth/ox-alpha", OX_ALPHA),
    // GLM 5.3 Flash is Ox Alpha shipped under its release id, so it keeps the
    // three-tier low/high/max ladder instead of the GLM-5.2 fallback.
    ("glm-5.3-flash", OX_ALPHA),
];

pub(super) fn resolve(id: &str) -> Option<ModelProfile> {
    PROFILES
        .iter()
        .find_map(|(candidate, profile)| (*candidate == id).then_some(*profile))
}

/// Any uncatalogued GLM id inherits the current 1M series window. Explicit
/// catalog entries above still win; later per-model configs override this.
pub(super) fn fallback(id: &str) -> Option<ModelProfile> {
    id.starts_with("glm").then_some(GLM_5_2)
}
