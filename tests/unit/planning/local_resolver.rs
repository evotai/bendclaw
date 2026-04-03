use std::path::PathBuf;
use std::sync::Arc;

use bendclaw::planning::LocalPromptResolver;
use bendclaw::planning::PromptRequestMeta;
use bendclaw::planning::PromptResolver;
use bendclaw::planning::PromptSeed;

fn noop_meta() -> PromptRequestMeta {
    PromptRequestMeta {
        channel_type: None,
        channel_chat_id: None,
        system_overlay: None,
        skill_overlay: None,
    }
}

#[tokio::test]
async fn local_resolver_returns_non_empty_prompt() {
    let resolver = LocalPromptResolver::new(
        PromptSeed::default(),
        Arc::new(vec![]),
        PathBuf::from("/tmp"),
    );
    let result = resolver.resolve(&noop_meta()).await.unwrap();
    assert!(!result.is_empty());
}

#[tokio::test]
async fn local_resolver_no_db_access() {
    // LocalPromptResolver must not panic or error without any DB
    let resolver = LocalPromptResolver::new(
        PromptSeed::default(),
        Arc::new(vec![]),
        PathBuf::from("/nonexistent"),
    );
    let result = resolver.resolve(&noop_meta()).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn local_resolver_system_overlay_included() {
    let resolver = LocalPromptResolver::new(
        PromptSeed::default(),
        Arc::new(vec![]),
        PathBuf::from("/tmp"),
    );
    let meta = PromptRequestMeta {
        system_overlay: Some("SYSTEM_OVERLAY_TEXT".to_string()),
        ..noop_meta()
    };
    let prompt = resolver.resolve(&meta).await.unwrap();
    assert!(prompt.contains("SYSTEM_OVERLAY_TEXT"));
}

#[tokio::test]
async fn local_resolver_skill_overlay_included() {
    let resolver = LocalPromptResolver::new(
        PromptSeed::default(),
        Arc::new(vec![]),
        PathBuf::from("/tmp"),
    );
    let meta = PromptRequestMeta {
        skill_overlay: Some("SKILL_OVERLAY_TEXT".to_string()),
        ..noop_meta()
    };
    let prompt = resolver.resolve(&meta).await.unwrap();
    assert!(prompt.contains("SKILL_OVERLAY_TEXT"));
}

#[tokio::test]
async fn local_resolver_channel_type_in_meta_does_not_panic() {
    let resolver = LocalPromptResolver::new(
        PromptSeed::default(),
        Arc::new(vec![]),
        PathBuf::from("/tmp"),
    );
    let meta = PromptRequestMeta {
        channel_type: Some("telegram".to_string()),
        channel_chat_id: Some("12345".to_string()),
        ..noop_meta()
    };
    let result = resolver.resolve(&meta).await;
    assert!(result.is_ok());
}
