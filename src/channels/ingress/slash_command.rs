use std::sync::Arc;

use super::dispatch_context::DispatchContext;
use crate::channels::model::account::ChannelAccount;
use crate::channels::runtime::diagnostics;
use crate::kernel::runtime::Runtime;
use crate::observability::log::channel_log;

/// Handle slash commands (/new, /clear). Returns true if handled.
pub(crate) async fn handle_slash_command(
    runtime: &Arc<Runtime>,
    account: &ChannelAccount,
    ctx: &mut DispatchContext,
    chat_id: &str,
    input: &str,
) -> bool {
    let trimmed = input.trim();

    if trimmed == "/clear" {
        if let Some(session) = runtime.sessions().get(&ctx.session_id) {
            session.clear_history();
        }
        channel_log!(info, "command", "clear_history",
            channel_type = %account.channel_type,
            account_id = %account.channel_account_id,
            chat_id,
            session = %ctx.session_id,
        );
        if let Some(ref ob) = ctx.outbound {
            let _ = ob
                .send_text(&account.config, chat_id, "Conversation history cleared.")
                .await;
        }
        return true;
    }

    if trimmed != "/new" {
        return false;
    }

    let old_session_id = ctx.session_id.clone();
    let new_session = match runtime
        .session_lifecycle()
        .start_new(&account.agent_id, &account.user_id, &ctx.base_key, "new")
        .await
    {
        Ok(session) => session,
        Err(error) => {
            diagnostics::log_channel_session_reset_failed(
                &account.agent_id,
                &account.channel_type,
                &account.channel_account_id,
                chat_id,
                &error,
            );
            if let Some(ref ob) = ctx.outbound {
                let _ = ob
                    .send_text(
                        &account.config,
                        chat_id,
                        "Failed to start a new conversation.",
                    )
                    .await;
            }
            return true;
        }
    };
    ctx.session_id = new_session.id.clone();

    channel_log!(info, "command", "new_session",
        channel_type = %account.channel_type,
        account_id = %account.channel_account_id,
        chat_id,
        old_session = %old_session_id,
        new_session = %new_session.id,
    );

    if let Some(ref ob) = ctx.outbound {
        let _ = ob
            .send_text(&account.config, chat_id, "New conversation started.")
            .await;
    }

    true
}
