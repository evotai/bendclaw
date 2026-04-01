use std::sync::Arc;

use crate::kernel::runtime::Runtime;
use crate::kernel::task::delivery::delivery_context::ChannelDeliveryContext;
use crate::kernel::task::diagnostics;
use crate::storage::dal::channel_account::repo::ChannelAccountRepo;
use crate::storage::dal::task::TaskDelivery;

/// If the task has channel delivery, append channel context to the prompt
/// so the LLM can use `channel_send` with the correct parameters.
pub async fn enrich_prompt_with_delivery(
    prompt: &str,
    delivery: &TaskDelivery,
    runtime: &Arc<Runtime>,
    agent_id: &str,
) -> String {
    match delivery {
        TaskDelivery::Channel {
            channel_account_id,
            chat_id,
        } => {
            match resolve_channel_delivery_context(runtime, agent_id, channel_account_id, chat_id)
                .await
            {
                Some(ctx) => format!(
                    "{prompt}\n\n\
                     [Channel context] When you need to send results, use channel_send with: \
                     channel_type=\"{}\", chat_id=\"{}\".",
                    ctx.channel_type, ctx.chat_id
                ),
                None => {
                    diagnostics::log_channel_context_unavailable(
                        agent_id,
                        channel_account_id,
                        chat_id,
                    );
                    format!(
                        "{prompt}\n\n\
                         [Channel context] Automatic delivery is configured by the system. \
                         If direct channel tools are unavailable, produce a final response and the system will deliver it."
                    )
                }
            }
        }
        _ => prompt.to_string(),
    }
}

pub(crate) async fn resolve_channel_delivery_context(
    runtime: &Arc<Runtime>,
    agent_id: &str,
    channel_account_id: &str,
    chat_id: &str,
) -> Option<ChannelDeliveryContext> {
    let pool = runtime.databases().agent_pool(agent_id).ok()?;
    let repo = ChannelAccountRepo::new(pool);
    let account = repo.load(channel_account_id).await.ok().flatten()?;
    if account.channel_type.trim().is_empty() {
        return None;
    }
    Some(ChannelDeliveryContext {
        channel_type: account.channel_type,
        chat_id: chat_id.to_string(),
    })
}
