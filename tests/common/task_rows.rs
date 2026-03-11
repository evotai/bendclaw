use bendclaw::storage::pool::QueryResponse;

#[derive(Clone)]
pub struct TaskRow {
    pub id: String,
    pub executor_instance_id: String,
    pub name: String,
    pub prompt: String,
    pub enabled: bool,
    pub status: String,
    pub schedule_json: String,
    pub delivery_json: String,
    pub last_error: Option<String>,
    pub delete_after_run: bool,
    pub run_count: i32,
    pub last_run_at: Option<String>,
    pub next_run_at: Option<String>,
    pub lease_token: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl TaskRow {
    pub fn every(id: &str, name: &str, enabled: bool) -> Self {
        Self {
            id: id.to_string(),
            executor_instance_id: "inst-1".to_string(),
            name: name.to_string(),
            prompt: "run report".to_string(),
            enabled,
            status: "idle".to_string(),
            schedule_json: r#"{"kind":"every","seconds":60}"#.to_string(),
            delivery_json: String::new(),
            last_error: None,
            delete_after_run: false,
            run_count: 0,
            last_run_at: None,
            next_run_at: Some("2026-03-11T00:00:00Z".to_string()),
            lease_token: None,
            created_at: "2026-03-10T00:00:00Z".to_string(),
            updated_at: "2026-03-10T00:00:00Z".to_string(),
        }
    }

    pub fn into_json_row(self) -> Vec<serde_json::Value> {
        vec![
            serde_json::Value::String(self.id),
            serde_json::Value::String(self.executor_instance_id),
            serde_json::Value::String(self.name),
            serde_json::Value::String(self.prompt),
            serde_json::Value::String(self.enabled.to_string()),
            serde_json::Value::String(self.status),
            serde_json::Value::String(self.schedule_json),
            serde_json::Value::String(self.delivery_json),
            serde_json::Value::String(self.last_error.unwrap_or_default()),
            serde_json::Value::String(self.delete_after_run.to_string()),
            serde_json::Value::String(self.run_count.to_string()),
            serde_json::Value::String(self.last_run_at.unwrap_or_default()),
            serde_json::Value::String(self.next_run_at.unwrap_or_default()),
            serde_json::Value::String(self.lease_token.unwrap_or_default()),
            serde_json::Value::String(self.created_at),
            serde_json::Value::String(self.updated_at),
        ]
    }
}

#[derive(Clone)]
pub struct TaskHistoryRow {
    pub id: String,
    pub task_id: String,
    pub run_id: Option<String>,
    pub task_name: String,
    pub schedule_json: String,
    pub prompt: String,
    pub status: String,
    pub output: Option<String>,
    pub error: Option<String>,
    pub duration_ms: Option<i32>,
    pub delivery_json: String,
    pub delivery_status: Option<String>,
    pub delivery_error: Option<String>,
    pub executed_by_instance_id: Option<String>,
    pub created_at: String,
}

impl TaskHistoryRow {
    pub fn ok(task_id: &str) -> Self {
        Self {
            id: "hist-1".to_string(),
            task_id: task_id.to_string(),
            run_id: Some("run-1".to_string()),
            task_name: "nightly-report".to_string(),
            schedule_json: r#"{"kind":"every","seconds":60}"#.to_string(),
            prompt: "run report".to_string(),
            status: "ok".to_string(),
            output: Some("done".to_string()),
            error: None,
            duration_ms: Some(1200),
            delivery_json: String::new(),
            delivery_status: None,
            delivery_error: None,
            executed_by_instance_id: Some("inst-1".to_string()),
            created_at: "2026-03-11T00:05:00Z".to_string(),
        }
    }

    pub fn into_json_row(self) -> Vec<serde_json::Value> {
        vec![
            serde_json::Value::String(self.id),
            serde_json::Value::String(self.task_id),
            serde_json::Value::String(self.run_id.unwrap_or_default()),
            serde_json::Value::String(self.task_name),
            serde_json::Value::String(self.schedule_json),
            serde_json::Value::String(self.prompt),
            serde_json::Value::String(self.status),
            serde_json::Value::String(self.output.unwrap_or_default()),
            serde_json::Value::String(self.error.unwrap_or_default()),
            serde_json::Value::String(self.duration_ms.map(|v| v.to_string()).unwrap_or_default()),
            serde_json::Value::String(self.delivery_json),
            serde_json::Value::String(self.delivery_status.unwrap_or_default()),
            serde_json::Value::String(self.delivery_error.unwrap_or_default()),
            serde_json::Value::String(self.executed_by_instance_id.unwrap_or_default()),
            serde_json::Value::String(self.created_at),
        ]
    }
}

pub fn task_query(rows: impl IntoIterator<Item = TaskRow>) -> QueryResponse {
    QueryResponse {
        id: String::new(),
        state: "Succeeded".to_string(),
        error: None,
        data: rows.into_iter().map(TaskRow::into_json_row).collect(),
        next_uri: None,
        final_uri: None,
        schema: Vec::new(),
    }
}

pub fn task_history_query(rows: impl IntoIterator<Item = TaskHistoryRow>) -> QueryResponse {
    QueryResponse {
        id: String::new(),
        state: "Succeeded".to_string(),
        error: None,
        data: rows
            .into_iter()
            .map(TaskHistoryRow::into_json_row)
            .collect(),
        next_uri: None,
        final_uri: None,
        schema: Vec::new(),
    }
}

#[allow(dead_code)]
pub fn quoted_values(sql: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = sql.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\'' {
            continue;
        }
        let mut value = String::new();
        while let Some(next) = chars.next() {
            if next == '\'' {
                if chars.peek() == Some(&'\'') {
                    value.push('\'');
                    chars.next();
                    continue;
                }
                break;
            }
            value.push(next);
        }
        out.push(value);
    }
    out
}
