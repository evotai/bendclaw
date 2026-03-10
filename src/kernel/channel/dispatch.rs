use std::sync::Arc;

use tokio_stream::StreamExt;

use crate::base::new_id;
use crate::kernel::channel::account::ChannelAccount;
use crate::kernel::channel::dispatcher::ChannelDispatcher;
use crate::kernel::channel::message::InboundEvent;
use crate::kernel::runtime::Runtime;
use crate::kernel::run::event::Delta;
use crate::kernel::run::event::Event;
use crate::storage::dal::channel_message::record::ChannelMessageRecord;
use crate::storage::dal::channel_message::repo::ChannelMessageRepo;

/// Dispatch a single inbound event through the full conversation pipeline.
/// Kernel-layer function — no service-layer dependencies.
pub async fn dispatch_inbound(runtime: &Arc<Runtime>, account: ChannelAccount, event: InboundEvent) {
    if let Err(e) = try_dispatch_inbound(runtime, &account, &event).await {
        tracing::error!(
            agent_id = %account.agent_id,
            channel_type = %account.channel_type,
            account_id = %account.channel_account_id,
            error = %e,
            "channel dispatch failed"
        );
    }
}

async fn try_dispatch_inbound(
    runtime: &Arc<Runtime>,
    account: &ChannelAccount,
    event: &InboundEvent,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (input, reply_ctx) = ChannelDispatcher::extract_input(event);
    let chat_id = reply_ctx
        .as_ref()
        .map(|r| r.chat_id.as_str())
        .unwrap_or("");

    if input.trim().is_empty() {
        return Ok(());
    }

    let session_key = ChannelDispatcher::session_key(
        &account.channel_type,
        &account.external_account_id,
        chat_id,
    );

    let pool = runtime.databases().agent_pool(&account.agent_id)?;
    let msg_repo = ChannelMessageRepo::new(pool.clone());

    // Record inbound message.
    if let InboundEvent::Message(msg) = event {
        let _ = msg_repo
            .insert(&ChannelMessageRecord {
                id: new_id(),
                channel_type: account.channel_type.clone(),
                account_id: account.external_account_id.clone(),
                chat_id: msg.chat_id.clone(),
                session_id: session_key.clone(),
                direction: "inbound".into(),
                sender_id: msg.sender_id.clone(),
                text: msg.text.clone(),
                platform_message_id: msg.message_id.clone(),
                run_id: String::new(),
                attachments: "[]".into(),
                created_at: String::new(),
            })
            .await;
    }

    // Get outbound interface.
    let outbound = runtime
        .channels()
        .get(&account.channel_type)
        .map(|e| e.plugin.outbound());

    // Send typing indicator.
    if let Some(ref ob) = outbound {
        let _ = ob.send_typing(&account.config, chat_id).await;
    }

    // Run the session.
    let session = runtime
        .get_or_create_session(&account.agent_id, &session_key, &account.user_id)
        .await?;

    let trace_id = new_id();
    let mut run_stream = session.run(&input, &trace_id, None).await?;
    let run_id = run_stream.run_id().to_string();

    let mut output_text = String::new();
    while let Some(ev) = run_stream.next().await {
        if let Event::StreamDelta(Delta::Text { content }) = &ev {
            output_text.push_str(content);
        }
    }
    let _ = run_stream.finish().await;

    if output_text.trim().is_empty() {
        return Ok(());
    }

    // Truncate to channel's max message length.
    let max_len = runtime
        .channels()
        .get(&account.channel_type)
        .map(|e| e.plugin.capabilities().max_message_len)
        .unwrap_or(4096);
    if output_text.len() > max_len {
        output_text.truncate(max_len);
    }

    // Send reply.
    let platform_msg_id = if let Some(ref ob) = outbound {
        ob.send_text(&account.config, chat_id, &output_text)
            .await
            .unwrap_or_default()
    } else {
        String::new()
    };

    // Record outbound message.
    let _ = msg_repo
        .insert(&ChannelMessageRecord {
            id: new_id(),
            channel_type: account.channel_type.clone(),
            account_id: account.external_account_id.clone(),
            chat_id: chat_id.to_string(),
            session_id: session_key,
            direction: "outbound".into(),
            sender_id: "agent".into(),
            text: output_text,
            platform_message_id: platform_msg_id,
            run_id,
            attachments: "[]".into(),
            created_at: String::new(),
        })
        .await;

    Ok(())
}
