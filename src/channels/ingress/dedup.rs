use crate::channels::model::account::ChannelAccount;
use crate::channels::model::message::InboundEvent;
use crate::channels::runtime::diagnostics;
use crate::storage::dal::channel_message::repo::ChannelMessageRepo;

/// Dedup check. Returns true if ANY event in the batch is a duplicate.
pub(crate) async fn is_duplicate(
    msg_repo: &ChannelMessageRepo,
    account: &ChannelAccount,
    events: &[InboundEvent],
) -> bool {
    for event in events {
        if let InboundEvent::Message(msg) = event {
            if !msg.message_id.is_empty()
                && msg_repo
                    .exists_by_platform_message_id(
                        &account.channel_type,
                        &account.external_account_id,
                        &msg.chat_id,
                        &msg.message_id,
                    )
                    .await
                    .unwrap_or_else(|e| {
                        diagnostics::log_channel_dedup_check_failed(
                            &msg.message_id,
                            &account.channel_type,
                            &e,
                        );
                        false
                    })
            {
                return true;
            }
        }
    }
    false
}
