use evotengine::provider::MockProvider;
use evotengine::Agent;
use evotengine::AgentEvent;
use evotengine::AgentMessage;
use evotengine::Message;

#[tokio::test]
async fn bounded_run_preserves_completion_and_context() -> Result<(), Box<dyn std::error::Error>> {
    let mut agent = Agent::new(MockProvider::text("answer"));
    let (_control, mut rx) = agent
        .submit_bounded(vec![AgentMessage::Llm(Message::user("hello"))])
        .await;
    assert_eq!(rx.max_capacity(), 64);
    let mut ended = false;
    while let Some(event) = rx.recv().await {
        ended |= matches!(event, AgentEvent::AgentEnd { .. });
    }
    agent.finish().await;
    assert!(ended);
    Ok(())
}

#[tokio::test]
async fn dropped_bounded_consumer_cancels_and_allows_finish(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut agent = Agent::new(MockProvider::text("answer"));
    let (control, rx) = agent
        .submit_bounded(vec![AgentMessage::Llm(Message::user("hello"))])
        .await;
    drop(rx);
    tokio::time::timeout(std::time::Duration::from_secs(3), agent.finish()).await?;
    assert!(control.is_cancelled());
    Ok(())
}
