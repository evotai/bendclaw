use super::channel_delivery::deliver_channel;
use super::webhook_delivery::deliver_webhook;
use crate::kernel::channels::ChannelRegistry;
use crate::storage::dal::task::TaskDelivery;
use crate::storage::dal::task::TaskRecord;
use crate::storage::Pool;

/// Dispatch delivery based on task configuration.
pub async fn deliver_result(
    channels: &ChannelRegistry,
    pool: &Pool,
    http_client: &reqwest::Client,
    task: &TaskRecord,
    status: &str,
    output: Option<&str>,
    error: Option<&str>,
) -> (Option<String>, Option<String>) {
    match &task.delivery {
        TaskDelivery::None => (None, None),
        TaskDelivery::Webhook { url } => {
            deliver_webhook(http_client, url, task, status, output, error).await
        }
        TaskDelivery::Channel {
            channel_account_id,
            chat_id,
        } => {
            deliver_channel(
                channels,
                pool,
                channel_account_id,
                chat_id,
                task,
                status,
                output,
                error,
            )
            .await
        }
    }
}
