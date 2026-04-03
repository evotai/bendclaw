//! Narrow extension points for the engine loop.
//!
//! Defined at the `run/` level (not inside `engine/`) because these types
//! are held by `SessionResources` and injected from the orchestration layer.
//! Engine is the consumer, not the owner.

use async_trait::async_trait;

use crate::sessions::Message;

// ── BeforeTurnHook ──

/// Decision returned by [`BeforeTurnHook::before_turn`].
pub enum TurnDecision {
    /// Proceed with the LLM call normally.
    Continue,
    /// Abort the entire run.
    Abort(String),
    /// Inject messages before the LLM call.
    InjectMessages(Vec<Message>),
}

/// Called before each LLM turn.
///
/// Use cases: dynamic prompt adjustment, budget checks, context injection,
/// loop abort.
#[async_trait]
pub trait BeforeTurnHook: Send + Sync {
    async fn before_turn(&self, iteration: u32, messages: &[Message]) -> TurnDecision;
}

// Arc delegation so SessionResources can hold Arc<dyn BeforeTurnHook>
// and Engine can consume Box<dyn BeforeTurnHook>.
#[async_trait]
impl BeforeTurnHook for std::sync::Arc<dyn BeforeTurnHook> {
    async fn before_turn(&self, iteration: u32, messages: &[Message]) -> TurnDecision {
        (**self).before_turn(iteration, messages).await
    }
}

// ── SteeringSource ──

/// Decision returned by [`SteeringSource::check_steering`].
pub enum SteeringDecision {
    /// Continue the normal loop.
    Continue,
    /// Inject messages and re-enter the LLM.
    Redirect(Vec<Message>),
}

/// Checked after tool execution to allow mid-run redirection.
///
/// Complements the existing inbox (push model) with a pull model:
/// Engine asks the source if there are pending steering messages.
#[async_trait]
pub trait SteeringSource: Send + Sync {
    async fn check_steering(&self, iteration: u32) -> SteeringDecision;
}

#[async_trait]
impl SteeringSource for std::sync::Arc<dyn SteeringSource> {
    async fn check_steering(&self, iteration: u32) -> SteeringDecision {
        (**self).check_steering(iteration).await
    }
}
