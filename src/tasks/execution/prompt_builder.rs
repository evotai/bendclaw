use std::sync::Arc;

use crate::kernel::runtime::Runtime;
use crate::storage::dal::task::TaskDelivery;
use crate::tasks::delivery::delivery_context::resolve_channel_delivery_context;
use crate::tasks::delivery::delivery_context::ChannelDeliveryContext;
use crate::tasks::diagnostics;

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
            let ctx =
                resolve_channel_delivery_context(runtime, agent_id, channel_account_id, chat_id)
                    .await;
            if ctx.is_none() {
                diagnostics::log_channel_context_unavailable(agent_id, channel_account_id, chat_id);
            }
            apply_channel_context(prompt, ctx.as_ref())
        }
        _ => prompt.to_string(),
    }
}

/// Pure prompt-assembly: given an optional resolved channel context,
/// return the final prompt string.
pub fn apply_channel_context(prompt: &str, ctx: Option<&ChannelDeliveryContext>) -> String {
    match ctx {
        Some(ctx) => format!(
            "{prompt}\n\n\
             [Channel context] When you need to send results, use channel_send with: \
             channel_type=\"{}\", chat_id=\"{}\".",
            ctx.channel_type, ctx.chat_id
        ),
        None => format!(
            "{prompt}\n\n\
             [Channel context] Automatic delivery is configured by the system. \
             If direct channel tools are unavailable, produce a final response and the system will deliver it."
        ),
    }
}
