use crate::kernel::channel::delivery::delivery_service::ChannelDeliveryService;
use crate::kernel::channel::ChannelRegistry;
use crate::storage::dal::channel_account::repo::ChannelAccountRepo;
use crate::storage::dal::task::TaskRecord;
use crate::storage::Pool;

#[allow(clippy::too_many_arguments)]
pub async fn deliver_channel(
    channels: &ChannelRegistry,
    pool: &Pool,
    channel_account_id: &str,
    chat_id: &str,
    task: &TaskRecord,
    status: &str,
    output: Option<&str>,
    error: Option<&str>,
) -> (Option<String>, Option<String>) {
    let repo = ChannelAccountRepo::new(pool.clone());
    let account = match repo.load(channel_account_id).await {
        Ok(Some(account)) => account,
        Ok(None) => {
            return (
                Some("failed".to_string()),
                Some(format!("channel account '{channel_account_id}' not found")),
            )
        }
        Err(e) => return (Some("failed".to_string()), Some(e.to_string())),
    };

    let entry = match channels.get(&account.channel_type) {
        Some(entry) => entry,
        None => {
            return (
                Some("failed".to_string()),
                Some(format!(
                    "channel plugin '{}' not registered",
                    account.channel_type
                )),
            )
        }
    };
    let outbound = entry.plugin.outbound();

    let text = render_delivery_text(task, status, output, error);
    match ChannelDeliveryService::deliver_text(&outbound, &account.config, chat_id, &text).await {
        Ok(_) => (Some("ok".to_string()), None),
        Err(e) => (Some("failed".to_string()), Some(e.to_string())),
    }
}

pub fn render_delivery_text(
    task: &TaskRecord,
    status: &str,
    output: Option<&str>,
    error: Option<&str>,
) -> String {
    let mut sections = vec![format!(
        "Task '{}' finished with status '{}'.",
        task.name, status
    )];
    if let Some(output) = output.filter(|value| !value.trim().is_empty()) {
        sections.push(output.to_string());
    }
    if let Some(error) = error.filter(|value| !value.trim().is_empty()) {
        sections.push(format!("Error: {error}"));
    }
    sections.join("\n\n")
}
