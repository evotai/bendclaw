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
use crate::mocks::llm::MockLLMProvider;

#[derive(Clone)]
struct VariableState {
    records: Arc<Mutex<Vec<VariableRecord>>>,
}

#[derive(Clone)]
struct VariableRecord {
    id: String,
    key: String,
    value: String,
    secret: bool,
    revoked: bool,
    user_id: String,
    scope: String,
    created_by: String,
    last_used_at: Option<String>,
    last_used_by: Option<String>,
    created_at: String,
    updated_at: String,
}

fn quoted_values(sql: &str) -> Vec<String> {
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

fn variable_rows(records: &[VariableRecord]) -> bendclaw::storage::pool::QueryResponse {
    let data = records
        .iter()
        .map(|record| {
            vec![
                serde_json::Value::String(record.id.clone()),
                serde_json::Value::String(record.key.clone()),
                serde_json::Value::String(record.value.clone()),
                serde_json::Value::String(record.secret.to_string()),
                serde_json::Value::String(record.revoked.to_string()),
                serde_json::Value::String(record.user_id.clone()),
                serde_json::Value::String(record.scope.clone()),
                serde_json::Value::String(record.created_by.clone()),
                serde_json::Value::String(record.last_used_at.clone().unwrap_or_default()),
                serde_json::Value::String(record.last_used_by.clone().unwrap_or_default()),
                serde_json::Value::String(record.created_at.clone()),
                serde_json::Value::String(record.updated_at.clone()),
            ]
        })
        .collect();
    bendclaw::storage::pool::QueryResponse {
        id: String::new(),
        state: "Succeeded".to_string(),
        error: None,
        data,
        next_uri: None,
        final_uri: None,
        schema: Vec::new(),
    }
}

#[tokio::test]
async fn variables_api_fast_crud_and_masking() -> Result<()> {
    let state = VariableState {
        records: Arc::new(Mutex::new(Vec::new())),
    };
    let fake_state = state.clone();
    let fake = FakeDatabend::new(move |sql, _database| {
        if sql.starts_with("CREATE ") || sql.starts_with("--") {
            return Ok(paged_rows(&[], None, None));
        }
        if sql.contains("evotai_meta.evotai_agents") {
            return Ok(paged_rows(&[], None, None));
        }
        let mut records = fake_state.records.lock().expect("variable state");
        if sql.starts_with("INSERT INTO evotai_meta.variables") {
            let values = quoted_values(sql);
            // Quoted values: id, key, value, user_id, scope, created_by
            // secret and revoked are unquoted booleans
            let secret = sql.contains(", true, ") || sql.contains(", TRUE, ");
            records.push(VariableRecord {
                id: values[0].clone(),
                key: values[1].clone(),
                value: values[2].clone(),
                secret,
                revoked: false,
                user_id: values[3].clone(),
                scope: values.get(4).cloned().unwrap_or("shared".into()),
                created_by: values.get(5).cloned().unwrap_or_default(),
                last_used_at: None,
                last_used_by: None,
                created_at: "2026-03-11T00:00:00Z".to_string(),
                updated_at: "2026-03-11T00:00:00Z".to_string(),
            });
            return Ok(paged_rows(&[], None, None));
        }
        if sql.contains("FROM evotai_meta.variables")
            && sql.contains("WHERE user_id =")
            && sql.contains("AND id =")
        {
            let values = quoted_values(sql);
            let user_id = &values[0];
            let id = &values[1];
            let found: Vec<_> = records
                .iter()
                .filter(|r| r.user_id == *user_id && r.id == *id)
                .cloned()
                .collect();
            return Ok(variable_rows(&found));
        }
        if sql.contains("FROM evotai_meta.variables") && sql.contains("WHERE user_id =") {
            let values = quoted_values(sql);
            let user_id = &values[0];
            let found: Vec<_> = records
                .iter()
                .filter(|r| r.user_id == *user_id)
                .cloned()
                .collect();
            return Ok(variable_rows(&found));
        }
        if sql.starts_with("UPDATE evotai_meta.variables SET") {
            let values = quoted_values(sql);
            // Quoted values: key, value, id, user_id
            let id = values[2].clone();
            if let Some(record) = records.iter_mut().find(|r| r.id == id) {
                record.key = values[0].clone();
                record.value = values[1].clone();
                record.secret = sql.contains("secret=true");
                record.revoked = sql.contains("revoked=true");
                record.updated_at = "2026-03-11T00:10:00Z".to_string();
            }
            return Ok(paged_rows(&[], None, None));
        }
        if sql.starts_with("DELETE FROM evotai_meta.variables") {
            let values = quoted_values(sql);
            // Quoted values: id, user_id
            let id = values[0].clone();
            records.retain(|r| r.id != id);
            return Ok(paged_rows(&[], None, None));
        }
        Ok(paged_rows(&[], None, None))
    });
    let prefix = format!(
        "test_fast_var_{}_",
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

    // CREATE
    let created = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/agents/{agent_id}/variables"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&serde_json::json!({
                    "key": "API_TOKEN",
                    "value": "secret-value",
                    "secret": true,
                    "revoked": false
                }))?))?,
        )
        .await?;
    assert_eq!(created.status(), StatusCode::OK);
    let created_body = json_body(created).await?;
    let var_id = created_body["id"]
        .as_str()
        .expect("variable id")
        .to_string();
    assert_eq!(created_body["value"], "****");

    // LIST
    let list = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/variables"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(list.status(), StatusCode::OK);
    let list_body = json_body(list).await?;
    assert_eq!(list_body[0]["key"], "API_TOKEN");
    assert_eq!(list_body[0]["value"], "****");

    // UPDATE
    let updated = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri(format!("/v1/agents/{agent_id}/variables/{var_id}"))
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&serde_json::json!({
                    "key": "API_TOKEN_V2",
                    "value": "rotated",
                    "secret": false,
                    "revoked": true
                }))?))?,
        )
        .await?;
    assert_eq!(updated.status(), StatusCode::OK);

    // GET
    let got = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/variables/{var_id}"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(got.status(), StatusCode::OK);
    let got_body = json_body(got).await?;
    assert_eq!(got_body["key"], "API_TOKEN_V2");
    assert_eq!(got_body["secret"], false);
    assert_eq!(got_body["revoked"], true);
    assert_eq!(got_body["value"], "rotated");

    // DELETE
    let deleted = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/agents/{agent_id}/variables/{var_id}"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(deleted.status(), StatusCode::OK);

    // GET after delete → 404
    let missing = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/agents/{agent_id}/variables/{var_id}"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    Ok(())
}
