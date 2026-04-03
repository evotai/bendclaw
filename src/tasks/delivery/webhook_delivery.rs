use std::time::Duration;

use crate::storage::dal::task::TaskRecord;

const WEBHOOK_TIMEOUT: Duration = Duration::from_secs(10);

pub async fn deliver_webhook(
    client: &reqwest::Client,
    url: &str,
    task: &TaskRecord,
    status: &str,
    output: Option<&str>,
    error: Option<&str>,
) -> (Option<String>, Option<String>) {
    let payload = serde_json::json!({
        "task_id": task.id,
        "task_name": task.name,
        "status": status,
        "output": output,
        "error": error,
    });

    match client
        .post(url)
        .timeout(WEBHOOK_TIMEOUT)
        .json(&payload)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => (Some("ok".to_string()), None),
        Ok(resp) => (
            Some("failed".to_string()),
            Some(format!("HTTP {}", resp.status())),
        ),
        Err(e) => (Some("failed".to_string()), Some(e.to_string())),
    }
}
