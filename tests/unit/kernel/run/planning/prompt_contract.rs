use std::path::PathBuf;
use std::sync::Arc;

use bendclaw::kernel::run::planning::LocalPromptResolver;
use bendclaw::kernel::run::planning::PromptRequestMeta;
use bendclaw::kernel::run::planning::PromptResolver;
use bendclaw::kernel::run::planning::PromptSeed;

fn noop_meta() -> PromptRequestMeta {
    PromptRequestMeta {
        channel_type: None,
        channel_chat_id: None,
        system_overlay: None,
        skill_overlay: None,
    }
}

fn local_resolver() -> impl PromptResolver {
    LocalPromptResolver::new(
        PromptSeed::default(),
        Arc::new(vec![]),
        PathBuf::from("/tmp"),
    )
}

#[tokio::test]
async fn prompt_resolver_returns_string() {
    let resolver = local_resolver();
    let result = resolver.resolve(&noop_meta()).await;
    assert!(result.is_ok());
    assert!(!result.unwrap().is_empty());
}

#[tokio::test]
async fn prompt_resolver_applies_system_overlay() {
    let resolver = local_resolver();
    let meta = PromptRequestMeta {
        system_overlay: Some("OVERLAY_MARKER".to_string()),
        ..noop_meta()
    };
    let prompt = resolver.resolve(&meta).await.unwrap();
    assert!(prompt.contains("OVERLAY_MARKER"));
}

#[tokio::test]
async fn prompt_resolver_applies_skill_overlay() {
    let resolver = local_resolver();
    let meta = PromptRequestMeta {
        skill_overlay: Some("SKILL_MARKER".to_string()),
        ..noop_meta()
    };
    let prompt = resolver.resolve(&meta).await.unwrap();
    assert!(prompt.contains("SKILL_MARKER"));
}

#[tokio::test]
async fn prompt_resolver_dyn_dispatch() {
    let resolver: Arc<dyn PromptResolver> = Arc::new(local_resolver());
    let result = resolver.resolve(&noop_meta()).await;
    assert!(result.is_ok());
}
