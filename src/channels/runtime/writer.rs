//! Background writer for channel messages — fire-and-forget persistence.

use crate::channels::runtime::diagnostics;
use crate::storage::dal::channel_message::record::ChannelMessageRecord;
use crate::storage::dal::channel_message::repo::ChannelMessageRepo;
use crate::writer::BackgroundWriter;

pub enum ChannelMessageOp {
    Insert {
        repo: ChannelMessageRepo,
        record: ChannelMessageRecord,
    },
}

pub type ChannelMessageWriter = BackgroundWriter<ChannelMessageOp>;

pub fn spawn_channel_message_writer() -> ChannelMessageWriter {
    BackgroundWriter::spawn("channel_message", 256, |op| async {
        match op {
            ChannelMessageOp::Insert { repo, record } => {
                if let Err(e) = repo.insert(&record).await {
                    diagnostics::log_channel_insert_failed(&e);
                }
            }
        }
        true
    })
}
