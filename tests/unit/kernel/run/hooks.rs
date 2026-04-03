use async_trait::async_trait;
use bendclaw::kernel::run::hooks::BeforeTurnHook;
use bendclaw::kernel::run::hooks::SteeringDecision;
use bendclaw::kernel::run::hooks::SteeringSource;
use bendclaw::kernel::run::hooks::TurnDecision;
use bendclaw::sessions::Message;

// ── BeforeTurnHook ──

struct AbortHook;

#[async_trait]
impl BeforeTurnHook for AbortHook {
    async fn before_turn(&self, _iteration: u32, _messages: &[Message]) -> TurnDecision {
        TurnDecision::Abort("budget exceeded".into())
    }
}

struct InjectHook {
    messages: Vec<Message>,
}

#[async_trait]
impl BeforeTurnHook for InjectHook {
    async fn before_turn(&self, _iteration: u32, _messages: &[Message]) -> TurnDecision {
        TurnDecision::InjectMessages(self.messages.clone())
    }
}

struct ContinueHook;

#[async_trait]
impl BeforeTurnHook for ContinueHook {
    async fn before_turn(&self, _iteration: u32, _messages: &[Message]) -> TurnDecision {
        TurnDecision::Continue
    }
}

#[test]
fn abort_hook_returns_abort() {
    let hook = AbortHook;
    let rt = tokio::runtime::Runtime::new().unwrap();
    let decision = rt.block_on(hook.before_turn(1, &[]));
    assert!(matches!(decision, TurnDecision::Abort(_)));
}

#[test]
fn inject_hook_returns_messages() {
    let hook = InjectHook {
        messages: vec![Message::user("injected")],
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let decision = rt.block_on(hook.before_turn(1, &[]));
    match decision {
        TurnDecision::InjectMessages(msgs) => {
            assert_eq!(msgs.len(), 1);
            assert_eq!(msgs[0].text(), "injected");
        }
        _ => panic!("expected InjectMessages"),
    }
}

#[test]
fn continue_hook_returns_continue() {
    let hook = ContinueHook;
    let rt = tokio::runtime::Runtime::new().unwrap();
    let decision = rt.block_on(hook.before_turn(1, &[]));
    assert!(matches!(decision, TurnDecision::Continue));
}

// ── SteeringSource ──

struct RedirectSource {
    messages: Vec<Message>,
}

#[async_trait]
impl SteeringSource for RedirectSource {
    async fn check_steering(&self, _iteration: u32) -> SteeringDecision {
        SteeringDecision::Redirect(self.messages.clone())
    }
}

struct NoopSource;

#[async_trait]
impl SteeringSource for NoopSource {
    async fn check_steering(&self, _iteration: u32) -> SteeringDecision {
        SteeringDecision::Continue
    }
}

#[test]
fn redirect_source_returns_messages() {
    let source = RedirectSource {
        messages: vec![Message::user("steer here")],
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let decision = rt.block_on(source.check_steering(1));
    match decision {
        SteeringDecision::Redirect(msgs) => {
            assert_eq!(msgs.len(), 1);
            assert_eq!(msgs[0].text(), "steer here");
        }
        _ => panic!("expected Redirect"),
    }
}

#[test]
fn noop_source_returns_continue() {
    let source = NoopSource;
    let rt = tokio::runtime::Runtime::new().unwrap();
    let decision = rt.block_on(source.check_steering(1));
    assert!(matches!(decision, SteeringDecision::Continue));
}

// ── Arc delegation ──

#[test]
fn arc_before_turn_hook_delegates() {
    let hook: std::sync::Arc<dyn BeforeTurnHook> = std::sync::Arc::new(AbortHook);
    let rt = tokio::runtime::Runtime::new().unwrap();
    let decision = rt.block_on(hook.before_turn(1, &[]));
    assert!(matches!(decision, TurnDecision::Abort(_)));
}

#[test]
fn arc_steering_source_delegates() {
    let source: std::sync::Arc<dyn SteeringSource> = std::sync::Arc::new(NoopSource);
    let rt = tokio::runtime::Runtime::new().unwrap();
    let decision = rt.block_on(source.check_steering(1));
    assert!(matches!(decision, SteeringDecision::Continue));
}
