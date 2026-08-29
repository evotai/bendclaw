use super::profile::ModelProfile;
use super::profile::ReasoningProfile;
use super::profile::BASE;
use crate::ThinkingLevel;

// GLM-5.3 always thinks (docs.z.ai): three effort tiers, low / high / max,
// with max as the shipped default. There is no Off tier.
const GLM_5_3_LEVELS: &[(ThinkingLevel, Option<&str>)] = &[
    (ThinkingLevel::Low, Some("low")),
    (ThinkingLevel::High, Some("high")),
    (ThinkingLevel::Max, Some("max")),
];
// GLM-5.3's Anthropic-compatible endpoint accepts `output_config.effort`
// alongside `thinking.type=enabled` — same dialect as Kimi.
const GLM_5_3_REASONING: ReasoningProfile = ReasoningProfile {
    levels: GLM_5_3_LEVELS,
    default: ThinkingLevel::Max,
    anthropic_wire: Some(super::super::capabilities::AnthropicThinkingWire::Enabled),
};

// 1M total window (docs.z.ai) minus the 131_072 output budget.
const GLM_5_3: ModelProfile = ModelProfile {
    max_input_tokens: 917_504,
    advertised_context_window: Some(1_000_000),
    max_output_tokens: 131_072,
    vision: false,
    reasoning: GLM_5_3_REASONING,
    ..BASE
};

/// Ox Alpha — pre-release GLM-family reasoning model (OpenRouter `stealth`
/// listing), shipped as GLM 5.3 Flash. Same effort ladder as GLM-5.3;
/// multimodal input (text + image).
const OX_ALPHA: ModelProfile = ModelProfile {
    advertised_context_window: Some(1_048_576),
    vision: true,
    ..GLM_5_3
};

#[rustfmt::skip]
const PROFILES: &[(&str, ModelProfile)] = &[
    ("glm-5.3",          GLM_5_3),
    // OpenRouter serves the model as "stealth/ox-alpha"; the bare id covers
    // direct Z.ai endpoints and vendor-prefixed specs ("zai/ox-alpha").
    ("ox-alpha",         OX_ALPHA),
    ("stealth/ox-alpha", OX_ALPHA),
    // GLM 5.3 Flash is Ox Alpha shipped under its release id.
    ("glm-5.3-flash",    OX_ALPHA),
];

pub(super) fn resolve(id: &str) -> Option<ModelProfile> {
    PROFILES
        .iter()
        .find_map(|(candidate, profile)| (*candidate == id).then_some(*profile))
}

/// Any uncatalogued GLM id inherits the current 1M series window and the
/// GLM-5.3 effort ladder. Explicit catalog entries above still win; later
/// per-model configs override this.
pub(super) fn fallback(id: &str) -> Option<ModelProfile> {
    id.starts_with("glm").then_some(GLM_5_3)
}
