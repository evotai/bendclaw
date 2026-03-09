use std::sync::Arc;

use anyhow::Result;

use crate::mocks::context::test_session;
use crate::mocks::llm::MockLLMProvider;

#[tokio::test]
async fn manager_insert_get_remove_roundtrip() -> Result<()> {
    let manager = bendclaw::kernel::session::SessionManager::new();
    let session = Arc::new(test_session(Arc::new(MockLLMProvider::with_text("ok"))).await?);
    let id = session.id.clone();

    manager.insert(session.clone());
    let got = manager
        .get(&id)
        .ok_or_else(|| std::io::Error::other("session missing"))?;
    assert_eq!(got.id, id);

    manager.remove(&id);
    assert!(manager.get(&id).is_none());
    Ok(())
}

#[tokio::test]
async fn manager_stats_reflects_session_counts() -> Result<()> {
    let manager = bendclaw::kernel::session::SessionManager::new();
    let s1 = Arc::new(test_session(Arc::new(MockLLMProvider::with_text("ok"))).await?);
    let s2 = Arc::new(test_session(Arc::new(MockLLMProvider::with_text("ok"))).await?);

    manager.insert(s1.clone());
    let stats1 = manager.stats();
    assert_eq!(stats1.total, 1);
    assert_eq!(stats1.idle, 1);
    assert_eq!(stats1.active, 0);

    manager.remove(&s1.id);
    manager.insert(s2.clone());
    let stats2 = manager.stats();
    assert_eq!(stats2.total, 1);
    assert_eq!(stats2.idle, 1);
    assert_eq!(stats2.active, 0);
    assert_eq!(stats2.sessions.len(), 1);
    Ok(())
}

#[tokio::test]
async fn manager_close_all_clears_sessions_and_can_suspend() -> Result<()> {
    let manager = bendclaw::kernel::session::SessionManager::new();
    let session = Arc::new(test_session(Arc::new(MockLLMProvider::with_text("ok"))).await?);
    manager.insert(session);
    assert!(manager.can_suspend());

    manager.close_all().await;

    let stats = manager.stats();
    assert_eq!(stats.total, 0);
    assert!(manager.can_suspend());
    Ok(())
}
