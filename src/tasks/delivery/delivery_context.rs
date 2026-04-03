use std::sync::Arc;

use crate::runtime::Runtime;
use crate::storage::dal::channel_account::repo::ChannelAccountRepo;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelDeliveryContext {
    pub channel_type: String,
    pub chat_id: String,
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
