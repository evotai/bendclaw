use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use async_trait::async_trait;

use super::circuit_breaker::CircuitBreaker;
use super::provider::LLMProvider;
use super::provider::LLMResponse;
use super::registry::ProviderRegistry;
use super::reliable::ReliableProvider;
use super::stream::ResponseStream;
use crate::base::ErrorCode;
use crate::base::Result;
use crate::llm::config::ProviderEndpoint;

/// Weighted provider pool with circuit-breaker failover.
///
/// Each slot wraps a `ReliableProvider` (per-request retries) and tracks
/// consecutive post-retry failures. When a slot's failure count reaches
/// the threshold, it is "tripped" (circuit open) and skipped for a
/// cooldown period. After cooldown, one probe request is allowed
/// (half-open); on success the circuit closes, on failure it re-trips.
///
/// Slots are tried in weight-descending order. Both `chat()` and
/// `chat_stream()` use each slot's own model — the `model` parameter
/// from the caller is ignored.
pub struct LLMRouter {
    slots: Vec<Slot>,
}

struct Slot {
    name: String,
    provider_name: String,
    provider: Arc<dyn LLMProvider>,
    model: String,
    temperature: f64,
    breaker: CircuitBreaker,
    input_price: f64,
    output_price: f64,
}

impl LLMRouter {
    /// Build a pool from an `LLMConfig` using the default provider registry.
    /// Handles chat providers with circuit-breaker failover.
    pub fn from_config(config: &super::config::LLMConfig) -> Result<Self> {
        let registry = ProviderRegistry::with_builtins();
        let chat: Vec<ProviderEndpoint> = config.providers.to_vec();
        Self::with_registry(
            &chat,
            &registry,
            config.max_retries,
            config.base_backoff_ms,
            config.circuit_breaker_threshold,
            Duration::from_secs(config.circuit_breaker_cooldown_secs),
        )
    }

    /// Build a pool from endpoint configs. Each endpoint is wrapped in
    /// `ReliableProvider` (default retries). Slots are sorted by weight
    /// descending.
    fn with_registry(
        endpoints: &[ProviderEndpoint],
        registry: &ProviderRegistry,
        max_retries: u32,
        base_backoff_ms: u64,
        failure_threshold: u32,
        cooldown: Duration,
    ) -> Result<Self> {
        if endpoints.is_empty() {
            return Ok(Self { slots: Vec::new() });
        }

        let mut slots_with_weight: Vec<(Slot, u32)> = endpoints
            .iter()
            .map(|ep| {
                let raw = registry.create(ep)?;
                let reliable: Arc<dyn LLMProvider> = Arc::new(
                    ReliableProvider::wrap(raw)
                        .max_retries(max_retries)
                        .base_backoff_ms(base_backoff_ms),
                );
                Ok((
                    Slot {
                        name: ep.name.clone(),
                        provider_name: ep.provider.clone(),
                        provider: reliable,
                        model: ep.model.clone(),
                        temperature: ep.temperature,
                        breaker: CircuitBreaker::new(failure_threshold, cooldown),
                        input_price: ep.input_price,
                        output_price: ep.output_price,
                    },
                    ep.weight,
                ))
            })
            .collect::<Result<Vec<_>>>()?;

        slots_with_weight.sort_by(|a, b| b.1.cmp(&a.1));
        let slots = slots_with_weight.into_iter().map(|(s, _)| s).collect();

        Ok(Self { slots })
    }
}

#[async_trait]
impl LLMProvider for LLMRouter {
    async fn chat(
        &self,
        _model: &str,
        messages: &[super::message::ChatMessage],
        tools: &[super::tool::ToolSchema],
        _temperature: f64,
    ) -> Result<LLMResponse> {
        let mut last_error = None;
        let start = Instant::now();

        if self.slots.is_empty() {
            return Err(ErrorCode::llm_request("no LLM providers configured"));
        }

        for slot in &self.slots {
            if !slot.breaker.is_available() {
                tracing::debug!(name = %slot.name, provider = %slot.provider_name, model = %slot.model, "slot tripped, skipping");
                continue;
            }

            match slot
                .provider
                .chat(&slot.model, messages, tools, slot.temperature)
                .await
            {
                Ok(resp) => {
                    slot.breaker.record_success();
                    let latency_ms = start.elapsed().as_millis() as u64;
                    let (prompt_tokens, completion_tokens) = resp
                        .usage
                        .as_ref()
                        .map(|u| (u.prompt_tokens, u.completion_tokens))
                        .unwrap_or((0, 0));
                    tracing::info!(
                        provider = %slot.provider_name,
                        model = %slot.model,
                        latency_ms,
                        prompt_tokens,
                        completion_tokens,
                        finish_reason = ?resp.finish_reason,
                        "llm request completed"
                    );
                    return Ok(resp);
                }
                Err(e) => {
                    slot.breaker.record_failure();
                    tracing::warn!(
                        name = %slot.name,
                        provider = %slot.provider_name,
                        model = %slot.model,
                        error = %e,
                        failures = slot.breaker.failure_count(),
                        "provider failed"
                    );
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            ErrorCode::llm_request(format!("all {} providers tripped", self.slots.len()))
        }))
    }

    fn chat_stream(
        &self,
        _model: &str,
        messages: &[super::message::ChatMessage],
        tools: &[super::tool::ToolSchema],
        _temperature: f64,
    ) -> ResponseStream {
        if self.slots.is_empty() {
            return ResponseStream::from_error(ErrorCode::llm_request(
                "no LLM providers configured",
            ));
        }

        for slot in &self.slots {
            if slot.breaker.is_available() {
                tracing::debug!(
                    provider = %slot.provider_name,
                    model = %slot.model,
                    "llm stream started"
                );
                return slot
                    .provider
                    .chat_stream(&slot.model, messages, tools, slot.temperature);
            }
        }

        // All tripped — use the first slot anyway (best effort)
        tracing::warn!("all providers tripped, using first slot as fallback");
        let slot = &self.slots[0];
        tracing::warn!(name = %slot.name, provider = %slot.provider_name, model = %slot.model, "falling back to first slot");
        slot.provider
            .chat_stream(&slot.model, messages, tools, slot.temperature)
    }

    fn pricing(&self, model: &str) -> Option<(f64, f64)> {
        // Return pricing from the first available slot whose model matches,
        // or the first available slot if no model match.
        let available: Vec<&Slot> = self
            .slots
            .iter()
            .filter(|s| s.breaker.is_available())
            .collect();
        let slots = if available.is_empty() {
            self.slots.iter().collect::<Vec<_>>()
        } else {
            available
        };

        if let Some(slot) = slots.iter().find(|s| s.model == model) {
            if slot.input_price > 0.0 || slot.output_price > 0.0 {
                return Some((slot.input_price, slot.output_price));
            }
        }
        // Fall back to first slot
        if let Some(slot) = slots.first() {
            if slot.input_price > 0.0 || slot.output_price > 0.0 {
                return Some((slot.input_price, slot.output_price));
            }
        }
        None
    }

    fn default_model(&self) -> &str {
        self.slots
            .first()
            .map(|s| s.model.as_str())
            .unwrap_or("unknown")
    }

    fn default_temperature(&self) -> f64 {
        self.slots.first().map(|s| s.temperature).unwrap_or(0.7)
    }
}
