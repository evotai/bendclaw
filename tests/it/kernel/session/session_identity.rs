use std::sync::Arc;

use anyhow::Result;

use crate::mocks::context::test_session;
use crate::mocks::llm::MockLLMProvider;

#[tokio::test]
async fn session_belongs_to_matches_exact_agent_and_user() -> Result<()> {
    let llm = Arc::new(MockLLMProvider::with_text("ok"));
    let session = test_session(llm).await?;

    assert!(session.belongs_to("a1", "u1"));
    assert!(!session.belongs_to("a2", "u1"));
    assert!(!session.belongs_to("a1", "u2"));
    assert!(!session.belongs_to("a2", "u2"));
    Ok(())
}
