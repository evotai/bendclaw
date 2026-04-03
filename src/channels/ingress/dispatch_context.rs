use std::sync::Arc;

use crate::channels::model::account::ChannelAccount;
use crate::channels::model::context::ChannelContext;
use crate::channels::runtime::channel_trait::ChannelOutbound;
use crate::kernel::runtime::Runtime;
use crate::storage::dal::channel_message::repo::ChannelMessageRepo;

pub(crate) struct DispatchContext {
    pub session_id: String,
    pub base_key: String,
    pub outbound: Option<Arc<dyn ChannelOutbound>>,
    pub msg_repo: ChannelMessageRepo,
    pub max_message_len: usize,
}

pub(crate) async fn resolve_dispatch_context(
    runtime: &Arc<Runtime>,
    account: &ChannelAccount,
    chat_id: &str,
) -> std::result::Result<DispatchContext, Box<dyn std::error::Error + Send + Sync>> {
    let base_key =
        ChannelContext::base_key(&account.channel_type, &account.external_account_id, chat_id);

    let outbound = runtime
        .channels()
        .get(&account.channel_type)
        .map(|e| e.plugin.outbound());

    let session_id = runtime
        .session_lifecycle()
        .resolve_active(&account.agent_id, &account.user_id, &base_key)
        .await?
        .id;

    let pool = runtime.databases().agent_pool(&account.agent_id)?;
    let msg_repo = ChannelMessageRepo::new(pool.clone());

    let caps = runtime
        .channels()
        .get(&account.channel_type)
        .map(|e| e.plugin.capabilities());
    let max_message_len = caps.as_ref().map(|c| c.max_message_len).unwrap_or(4096);

    Ok(DispatchContext {
        session_id,
        base_key,
        outbound,
        msg_repo,
        max_message_len,
    })
}
