use anyhow::Result;
use bendclaw::storage::ChannelMessageRepo;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;

fn msg_row(id: &str) -> Vec<serde_json::Value> {
    vec![
        id,
        "slack",
        "acc-1",
        "chat-1",
        "sess-1",
        "inbound",
        "sender-1",
        "hello",
        "pm-1",
        "run-1",
        "[]",
        "2026-03-11T00:00:00Z",
    ]
    .into_iter()
    .map(|s| serde_json::Value::String(s.to_string()))
    .collect()
}

fn msg_response(id: &str) -> bendclaw::storage::pool::QueryResponse {
    bendclaw::storage::pool::QueryResponse {
        id: String::new(),
        state: "Succeeded".into(),
        error: None,
        data: vec![msg_row(id)],
        next_uri: None,
        final_uri: None,
        schema: Vec::new(),
    }
}

#[tokio::test]
async fn channel_message_insert_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.starts_with("INSERT INTO channel_messages"));
        assert!(sql.contains("NOW()"));
        Ok(paged_rows(&[], None, None))
    });
    let repo = ChannelMessageRepo::new(fake.pool());
    let record = bendclaw::storage::ChannelMessageRecord {
        id: "cm-1".into(),
        channel_type: "slack".into(),
        account_id: "acc-1".into(),
        chat_id: "chat-1".into(),
        session_id: "sess-1".into(),
        direction: "inbound".into(),
        sender_id: "sender-1".into(),
        text: "hello".into(),
        platform_message_id: "pm-1".into(),
        run_id: "run-1".into(),
        attachments: "[]".into(),
        created_at: String::new(),
    };
    repo.insert(&record).await?;
    assert_eq!(fake.calls().len(), 1);
    Ok(())
}

#[tokio::test]
async fn channel_message_list_by_chat_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.contains("WHERE channel_type = 'slack' AND chat_id = 'chat-1'"));
        assert!(sql.contains("ORDER BY created_at DESC"));
        Ok(msg_response("cm-1"))
    });
    let repo = ChannelMessageRepo::new(fake.pool());
    let msgs = repo.list_by_chat("slack", "chat-1", 50).await?;
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].text, "hello");
    Ok(())
}

#[tokio::test]
async fn channel_message_list_by_session_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.contains("WHERE session_id = 'sess-1'"));
        assert!(sql.contains("ORDER BY created_at DESC"));
        Ok(msg_response("cm-1"))
    });
    let repo = ChannelMessageRepo::new(fake.pool());
    let msgs = repo.list_by_session("sess-1", 50).await?;
    assert_eq!(msgs.len(), 1);
    Ok(())
}

#[tokio::test]
async fn channel_message_exists_by_platform_message_id_returns_true_when_found() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.starts_with("SELECT COUNT(*)"));
        assert!(sql.contains("channel_type = 'slack'"));
        assert!(sql.contains("account_id = 'acc-1'"));
        assert!(sql.contains("chat_id = 'chat-1'"));
        assert!(sql.contains("platform_message_id = 'pm-42'"));
        assert!(sql.contains("direction = 'inbound'"));
        Ok(paged_rows(&[&["1"]], None, None))
    });
    let repo = ChannelMessageRepo::new(fake.pool());
    let exists = repo
        .exists_by_platform_message_id("slack", "acc-1", "chat-1", "pm-42")
        .await?;
    assert!(exists);
    Ok(())
}

#[tokio::test]
async fn channel_message_exists_by_platform_message_id_returns_false_when_not_found() -> Result<()>
{
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.starts_with("SELECT COUNT(*)"));
        Ok(paged_rows(&[&["0"]], None, None))
    });
    let repo = ChannelMessageRepo::new(fake.pool());
    let exists = repo
        .exists_by_platform_message_id("slack", "acc-1", "chat-1", "pm-99")
        .await?;
    assert!(!exists);
    Ok(())
}
