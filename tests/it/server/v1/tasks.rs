use std::sync::Arc;
use std::sync::Mutex;

use anyhow::Result;
use axum::body::Body;
use axum::http::Request;
use axum::http::StatusCode;
use tower::ServiceExt;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;
use crate::common::setup::app_with_root_pool_and_llm;
use crate::common::setup::json_body;
use crate::common::setup::uid;
use crate::common::task_rows::quoted_values;
use crate::common::task_rows::task_history_query;
use crate::common::task_rows::task_query;
use crate::common::task_rows::TaskHistoryRow;
use crate::common::task_rows::TaskRow;
use crate::mocks::llm::MockLLMProvider;

#[derive(Clone)]
struct TaskState {
    records: Arc<Mutex<Vec<TaskRow>>>,
    history: Arc<Mutex<Vec<TaskHistoryRow>>>,
}

#[tokio::test]
async fn tasks_api_create_accepts_channel_delivery() -> Result<()> {
    let saw_insert = Arc::new(Mutex::new(false));
    let saw_insert_for_fake = Arc::clone(&saw_insert);
    let fake = FakeDatabend::new(move |sql, _database| {
        if sql.starts_with("INSERT INTO tasks") {
            *saw_insert_for_fake.lock().expect("insert marker") = true;
            assert!(sql.contains("\"kind\":\"channel\""));
            assert!(sql.contains("\"channel_account_id\":\"channel-1\""));
            assert!(sql.contains("\"chat_id\":\"chat-42\""));
        }
        Ok(paged_rows(&[], None, None))
    });
    let prefix = format!(
        "test_fast_task_delivery_{}_",
        ulid::Ulid::new().to_string().to_lowercase()
    );
    let app = app_with_root_pool_and_llm(
        fake.pool(),
        "http://fake.local/v1",
        "",
        "default",
        &prefix,
        Arc::new(MockLLMProvider::with_text("ok")),
    )
    .await?;
    let agent_id = uid("agent");
    let user = uid("user");

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/tasks"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&serde_json::json!({
                    "name": "notify-report",
                    "prompt": "run report",
                    "schedule": {
                        "kind": "every",
                        "seconds": 60
                    },
                    "delivery": {
                        "kind": "channel",
                        "channel_account_id": "channel-1",
                        "chat_id": "chat-42"
                    }
                }))?))?,
        )
        .await?;

    assert_eq!(response.status(), StatusCode::OK);
    let body = json_body(response).await?;
    assert_eq!(body["delivery"]["kind"], "channel");
    assert_eq!(body["delivery"]["channel_account_id"], "channel-1");
    assert_eq!(body["delivery"]["chat_id"], "chat-42");
    assert!(*saw_insert.lock().expect("insert marker"));
    Ok(())
}

#[tokio::test]
async fn tasks_api_fast_create_list_and_toggle() -> Result<()> {
    let state = TaskState {
        records: Arc::new(Mutex::new(Vec::new())),
        history: Arc::new(Mutex::new(Vec::new())),
    };
    let fake_state = state.clone();
    let fake = FakeDatabend::new(move |sql, _database| {
        let mut records = fake_state.records.lock().expect("task state");
        if sql.starts_with("INSERT INTO tasks") {
            let values = quoted_values(sql);
            records.push(TaskRow {
                id: values[0].clone(),
                node_id: values[1].clone(),
                name: values[2].clone(),
                prompt: values[3].clone(),
                enabled: true,
                status: values[4].clone(),
                schedule_json: r#"{"kind":"every","seconds":60}"#.to_string(),
                delivery_json: String::new(),
                user_id: String::new(),
                scope: String::new(),
                created_by: String::new(),
                last_error: None,
                delete_after_run: false,
                run_count: 0,
                last_run_at: None,
                next_run_at: Some("2026-03-11T00:00:00Z".to_string()),
                lease_token: None,
                lease_node_id: None,
                lease_expires_at: None,
                created_at: "2026-03-10T00:00:00Z".to_string(),
                updated_at: "2026-03-10T00:00:00Z".to_string(),
            });
            return Ok(paged_rows(&[], None, None));
        }
        if sql.starts_with("SELECT COUNT(*) FROM tasks") {
            let count = records.len().to_string();
            return Ok(paged_rows(&[&[count.as_str()]], None, None));
        }
        if sql.starts_with("SELECT id, node_id") && sql.contains("WHERE id = ") {
            let id = quoted_values(sql).pop().unwrap_or_default();
            let found: Vec<_> = records
                .iter()
                .filter(|record| record.id == id)
                .cloned()
                .collect();
            return Ok(task_query(found));
        }
        if sql.starts_with("SELECT id, node_id") {
            let mut all = records.clone();
            all.reverse();
            return Ok(task_query(all));
        }
        if sql.starts_with("UPDATE tasks SET enabled = NOT enabled") {
            let id = quoted_values(sql).pop().unwrap_or_default();
            if let Some(record) = records.iter_mut().find(|record| record.id == id) {
                record.enabled = !record.enabled;
                record.updated_at = "2026-03-11T00:10:00Z".to_string();
            }
            return Ok(paged_rows(&[], None, None));
        }
        Ok(paged_rows(&[], None, None))
    });
    let prefix = format!(
        "test_fast_task_{}_",
        ulid::Ulid::new().to_string().to_lowercase()
    );
    let app = app_with_root_pool_and_llm(
        fake.pool(),
        "http://fake.local/v1",
        "",
        "default",
        &prefix,
        Arc::new(MockLLMProvider::with_text("ok")),
    )
    .await?;
    let agent_id = uid("agent");
    let user = uid("user");

    let created = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/tasks"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&serde_json::json!({
                    "name": "nightly-report",
                    "prompt": "run report",
                    "schedule": {
                        "kind": "every",
                        "seconds": 60
                    }
                }))?))?,
        )
        .await?;
    assert_eq!(created.status(), StatusCode::OK);
    let created_body = json_body(created).await?;
    let task_id = created_body["id"].as_str().expect("task id").to_string();
    assert_eq!(created_body["name"], "nightly-report");
    assert_eq!(created_body["schedule"]["kind"], "every");

    let list = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/tasks"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(list.status(), StatusCode::OK);
    let list_body = json_body(list).await?;
    assert_eq!(list_body["data"][0]["id"], task_id);
    assert_eq!(list_body["data"][0]["enabled"], true);

    let toggled = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/tasks/{task_id}/toggle"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(toggled.status(), StatusCode::OK);
    let toggled_body = json_body(toggled).await?;
    assert_eq!(toggled_body["enabled"], false);
    Ok(())
}

#[tokio::test]
async fn tasks_api_fast_update_delete_and_history() -> Result<()> {
    let state = TaskState {
        records: Arc::new(Mutex::new(vec![TaskRow {
            id: "task-1".to_string(),
            node_id: "test_instance".to_string(),
            name: "nightly-report".to_string(),
            prompt: "run report".to_string(),
            enabled: true,
            status: "idle".to_string(),
            schedule_json: r#"{"kind":"every","seconds":60}"#.to_string(),
            delivery_json: String::new(),
            user_id: String::new(),
            scope: String::new(),
            created_by: String::new(),
            last_error: None,
            delete_after_run: false,
            run_count: 0,
            last_run_at: None,
            next_run_at: Some("2026-03-11T00:00:00Z".to_string()),
            lease_token: None,
            lease_node_id: None,
            lease_expires_at: None,
            created_at: "2026-03-10T00:00:00Z".to_string(),
            updated_at: "2026-03-10T00:00:00Z".to_string(),
        }])),
        history: Arc::new(Mutex::new(vec![TaskHistoryRow::ok("task-1")])),
    };
    let fake_state = state.clone();
    let fake = FakeDatabend::new(move |sql, _database| {
        if sql.starts_with("SELECT COUNT(*) FROM task_history") {
            let count = fake_state
                .history
                .lock()
                .expect("task history")
                .len()
                .to_string();
            return Ok(paged_rows(&[&[count.as_str()]], None, None));
        }
        if sql.starts_with("SELECT COUNT(*) FROM tasks") {
            let count = fake_state
                .records
                .lock()
                .expect("task state")
                .len()
                .to_string();
            return Ok(paged_rows(&[&[count.as_str()]], None, None));
        }
        if sql.starts_with("SELECT id, task_id, run_id") {
            let history = fake_state.history.lock().expect("task history").clone();
            return Ok(task_history_query(history));
        }

        let mut records = fake_state.records.lock().expect("task state");
        if sql.starts_with("SELECT id, node_id") && sql.contains("WHERE id = ") {
            let id = quoted_values(sql).pop().unwrap_or_default();
            let found: Vec<_> = records
                .iter()
                .filter(|record| record.id == id)
                .cloned()
                .collect();
            return Ok(task_query(found));
        }
        if sql.starts_with("UPDATE tasks SET ") {
            assert!(sql.contains("name = 'updated-report'"));
            assert!(sql.contains("enabled = false"));
            let id = quoted_values(sql).last().cloned().unwrap_or_default();
            if let Some(record) = records.iter_mut().find(|record| record.id == id) {
                record.name = "updated-report".to_string();
                record.enabled = false;
                record.updated_at = "2026-03-11T00:10:00Z".to_string();
            }
            return Ok(paged_rows(&[], None, None));
        }
        if sql.starts_with("DELETE FROM tasks WHERE id = ") {
            let id = quoted_values(sql).pop().unwrap_or_default();
            records.retain(|record| record.id != id);
            return Ok(paged_rows(&[], None, None));
        }
        Ok(paged_rows(&[], None, None))
    });
    let prefix = format!(
        "test_fast_task_{}_",
        ulid::Ulid::new().to_string().to_lowercase()
    );
    let app = app_with_root_pool_and_llm(
        fake.pool(),
        "http://fake.local/v1",
        "",
        "default",
        &prefix,
        Arc::new(MockLLMProvider::with_text("ok")),
    )
    .await?;
    let agent_id = uid("agent");
    let user = uid("user");

    let updated = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/v1/agents/{agent_id}/tasks/task-1"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&serde_json::json!({
                    "name": "updated-report",
                    "enabled": false
                }))?))?,
        )
        .await?;
    assert_eq!(updated.status(), StatusCode::OK);
    let updated_body = json_body(updated).await?;
    assert_eq!(updated_body["name"], "updated-report");
    assert_eq!(updated_body["enabled"], false);

    let history = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/tasks/task-1/history"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(history.status(), StatusCode::OK);
    let history_body = json_body(history).await?;
    assert_eq!(history_body["data"][0]["id"], "hist-1");
    assert_eq!(history_body["data"][0]["status"], "ok");
    assert_eq!(history_body["data"][0]["duration_ms"], 1200);

    let deleted = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/agents/{agent_id}/tasks/task-1"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(deleted.status(), StatusCode::OK);
    let deleted_body = json_body(deleted).await?;
    assert_eq!(deleted_body["deleted"], "task-1");

    let list = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/tasks"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(list.status(), StatusCode::OK);
    let list_body = json_body(list).await?;
    assert_eq!(list_body["data"].as_array().map(Vec::len), Some(0));
    Ok(())
}
