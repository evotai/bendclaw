use crate::base::new_id;
use crate::kernel::channels::model::account::ChannelAccount;
use crate::kernel::channels::model::message::InboundEvent;
use crate::kernel::channels::runtime::writer::ChannelMessageOp;
use crate::kernel::runtime::Runtime;
use crate::storage::dal::channel_message::record::ChannelMessageRecord;
use crate::storage::dal::channel_message::repo::ChannelMessageRepo;

/// Record all inbound messages (fire-and-forget).
pub(crate) fn record_inbound(
    runtime: &Runtime,
    msg_repo: &ChannelMessageRepo,
    account: &ChannelAccount,
    session_id: &str,
    events: &[InboundEvent],
) {
    for event in events {
        if let InboundEvent::Message(msg) = event {
            runtime
                .channel_message_writer
                .send(ChannelMessageOp::Insert {
                    repo: msg_repo.clone(),
                    record: ChannelMessageRecord {
                        id: new_id(),
                        channel_type: account.channel_type.clone(),
                        account_id: account.external_account_id.clone(),
                        chat_id: msg.chat_id.clone(),
                        session_id: session_id.to_string(),
                        direction: "inbound".into(),
                        sender_id: msg.sender_id.clone(),
                        text: msg.text.clone(),
                        platform_message_id: msg.message_id.clone(),
                        run_id: String::new(),
                        attachments: "[]".into(),
                        created_at: String::new(),
                    },
                });
        }
    }
}
