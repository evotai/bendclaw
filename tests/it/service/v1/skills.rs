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
use crate::common::setup::setup_agent;
use crate::common::setup::uid;
use crate::mocks::llm::MockLLMProvider;

#[derive(Clone)]
struct SkillRecord {
    name: String,
    version: String,
    scope: String,
    source: String,
    user_id: String,
    created_by: String,
    description: String,
    timeout: u64,
    executable: bool,
    content: String,
    files: Vec<(String, String)>,
}

fn skill_rows(records: &[SkillRecord]) -> bendclaw::storage::pool::QueryResponse {
    let data = records
        .iter()
        .map(|r| {
            vec![
                serde_json::Value::String(r.name.clone()),
                serde_json::Value::String(r.version.clone()),
                serde_json::Value::String(r.scope.clone()),
                serde_json::Value::String(r.source.clone()),
                serde_json::Value::String(r.user_id.clone()),
                serde_json::Value::String(r.created_by.clone()),
                serde_json::Value::String(r.description.clone()),
                serde_json::Value::String(r.timeout.to_string()),
                serde_json::Value::String(if r.executable { "true" } else { "false" }.into()),
                serde_json::Value::String(r.content.clone()),
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

fn file_rows(files: &[(String, String)]) -> bendclaw::storage::pool::QueryResponse {
    let data = files
        .iter()
        .map(|(path, body)| {
            vec![
                serde_json::Value::String(path.clone()),
                serde_json::Value::String(body.clone()),
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

#[tokio::test]
async fn create_skill_overwrites_same_name_within_agent_fast() -> Result<()> {
    let skills: Arc<Mutex<Vec<SkillRecord>>> = Arc::new(Mutex::new(Vec::new()));
    let fake_skills = skills.clone();

    let fake = FakeDatabend::new(move |sql, _database| {
        if sql.starts_with("CREATE ") || sql.starts_with("--") {
            return Ok(paged_rows(&[], None, None));
        }
        if sql.contains("evotai_meta.evotai_agents") || sql.contains("agent_config") {
            return Ok(paged_rows(&[], None, None));
        }
        if sql.contains("resource_subscriptions") {
            return Ok(paged_rows(&[], None, None));
        }

        let mut records = fake_skills.lock().expect("skill state");

        // DELETE (part of save's remove-then-insert)
        if sql.starts_with("DELETE FROM evotai_meta.skills") {
            let vals = quoted_values(sql);
            if vals.len() >= 2 {
                let name = &vals[0];
                let user_id = &vals[1];
                records.retain(|r| !(r.name == *name && r.user_id == *user_id));
            }
            return Ok(paged_rows(&[], None, None));
        }
        if sql.starts_with("DELETE FROM evotai_meta.skill_files") {
            return Ok(paged_rows(&[], None, None));
        }

        // INSERT skill
        // Quoted values: name, version, scope, source, user_id, created_by, description, content, sha256
        // Unquoted: timeout (number), executable (TRUE/FALSE), enabled (TRUE)
        if sql.starts_with("INSERT INTO evotai_meta.skills") {
            let vals = quoted_values(sql);
            let executable = sql.contains(", TRUE, TRUE, '");
            records.push(SkillRecord {
                name: vals[0].clone(),
                version: vals[1].clone(),
                scope: vals[2].clone(),
                source: vals[3].clone(),
                user_id: vals[4].clone(),
                created_by: vals[5].clone(),
                description: vals[6].clone(),
                timeout: 30,
                executable,
                content: vals.get(7).cloned().unwrap_or_default(),
                files: Vec::new(),
            });
            return Ok(paged_rows(&[], None, None));
        }

        // INSERT file
        if sql.starts_with("INSERT INTO evotai_meta.skill_files") {
            let vals = quoted_values(sql);
            let skill_name = &vals[0];
            if let Some(rec) = records.iter_mut().find(|r| r.name == *skill_name) {
                rec.files.push((vals[3].clone(), vals[4].clone()));
            }
            return Ok(paged_rows(&[], None, None));
        }

        // SELECT skills (list)
        if sql.contains("FROM evotai_meta.skills")
            && sql.contains("WHERE user_id =")
            && !sql.contains("AND name =")
        {
            let vals = quoted_values(sql);
            let user_id = &vals[0];
            let found: Vec<_> = records
                .iter()
                .filter(|r| r.user_id == *user_id)
                .cloned()
                .collect();
            return Ok(skill_rows(&found));
        }

        // SELECT skill (get by name)
        if sql.contains("FROM evotai_meta.skills") && sql.contains("AND name =") {
            let vals = quoted_values(sql);
            let user_id = &vals[0];
            let name = &vals[1];
            let found: Vec<_> = records
                .iter()
                .filter(|r| r.user_id == *user_id && r.name == *name)
                .cloned()
                .collect();
            return Ok(skill_rows(&found));
        }

        // SELECT files
        if sql.contains("FROM evotai_meta.skill_files") {
            let vals = quoted_values(sql);
            let skill_name = &vals[0];
            if let Some(rec) = records.iter().find(|r| r.name == *skill_name) {
                return Ok(file_rows(&rec.files));
            }
            return Ok(paged_rows(&[], None, None));
        }

        Ok(paged_rows(&[], None, None))
    });

    let prefix = format!(
        "test_fast_skill_{}_",
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
    let skill_name = "report-skill";
    setup_agent(&app, &agent_id, &user).await?;

    let first = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/skills")
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&serde_json::json!({
                    "name": skill_name,
                    "description": "first version",
                    "content": "first body",
                    "files": [{
                        "path": "references/old.md",
                        "body": "# old"
                    }]
                }))?))?,
        )
        .await?;
    assert_eq!(first.status(), StatusCode::OK);

    let second = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/skills")
                .header("content-type", "application/json")
                .header("x-user-id", &user)
                .body(Body::from(serde_json::to_vec(&serde_json::json!({
                    "name": skill_name,
                    "description": "second version",
                    "content": "second body",
                    "files": [{
                        "path": "references/new.md",
                        "body": "# new"
                    }]
                }))?))?,
        )
        .await?;
    assert_eq!(second.status(), StatusCode::OK);

    let get_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/skills/{skill_name}"))
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(get_resp.status(), StatusCode::OK);
    let skill = json_body(get_resp).await?;
    assert_eq!(skill["description"], "second version");
    assert_eq!(skill["content"], "second body");
    assert_eq!(skill["created_by"], user);
    assert_eq!(skill["files"][0]["path"], "references/new.md");

    let list_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/skills")
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(list_resp.status(), StatusCode::OK);
    let skills = json_body(list_resp).await?;
    let items = skills.as_array().expect("skill list should be an array");
    let matches: Vec<_> = items
        .iter()
        .filter(|skill| skill["name"] == skill_name)
        .collect();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0]["description"], "second version");
    assert_eq!(matches[0]["files"][0]["path"], "references/new.md");
    Ok(())
}

#[tokio::test]
async fn subscribed_skill_visible_in_list_and_gettable_by_namespaced_key() -> Result<()> {
    use crate::common::setup::app_with_workspace;

    let fake = FakeDatabend::new(|sql, _database| {
        if sql.contains("resource_subscriptions") {
            return Ok(paged_rows(&[], None, None));
        }
        Ok(paged_rows(&[], None, None))
    });
    let prefix = format!(
        "test_sub_skill_{}_",
        ulid::Ulid::new().to_string().to_lowercase()
    );
    let (app, workspace_root) = app_with_workspace(
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
    setup_agent(&app, &agent_id, &user).await?;

    // Write a subscribed skill directly to disk (alice owns "report", user subscribes)
    let skill = bendclaw::kernel::skills::model::skill::Skill {
        name: "report".to_string(),
        version: "1.0.0".to_string(),
        description: "alice report".to_string(),
        scope: bendclaw::kernel::skills::model::skill::SkillScope::Shared,
        source: bendclaw::kernel::skills::model::skill::SkillSource::Agent,
        user_id: "alice".to_string(),
        created_by: Some("alice".to_string()),
        last_used_by: None,
        timeout: 30,
        executable: false,
        parameters: vec![],
        content: "# Alice Report".to_string(),
        files: vec![],
        requires: None,
        manifest: None,
    };
    bendclaw::kernel::skills::sources::remote::writer::write_subscribed_skill(
        &workspace_root,
        &user,
        "alice",
        &skill,
    );

    // GET /v1/skills/alice/report should return the subscribed skill
    let get_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/skills/alice/report")
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(get_resp.status(), StatusCode::OK);
    let body = json_body(get_resp).await?;
    assert_eq!(body["name"], "alice/report");
    assert_eq!(body["owner_id"], "alice");
    assert_eq!(body["description"], "alice report");
    assert_eq!(body["content"], "# Alice Report");

    // list should include the subscribed skill with namespaced name
    let list_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/skills")
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(list_resp.status(), StatusCode::OK);
    let skills = json_body(list_resp).await?;
    let items = skills.as_array().expect("skill list should be an array");
    let sub_matches: Vec<_> = items
        .iter()
        .filter(|s| s["name"] == "alice/report")
        .collect();
    assert_eq!(sub_matches.len(), 1);
    assert_eq!(sub_matches[0]["owner_id"], "alice");

    Ok(())
}

#[tokio::test]
async fn delete_subscribed_skill_triggers_unsubscribe() -> Result<()> {
    use crate::common::setup::app_with_workspace;

    let unsub_called = Arc::new(Mutex::new(false));
    let unsub_flag = unsub_called.clone();

    let fake = FakeDatabend::new(move |sql, _database| {
        if sql.starts_with("DELETE FROM evotai_meta.resource_subscriptions") {
            *unsub_flag.lock().expect("lock") = true;
        }
        Ok(paged_rows(&[], None, None))
    });
    let prefix = format!(
        "test_del_sub_{}_",
        ulid::Ulid::new().to_string().to_lowercase()
    );
    let (app, workspace_root) = app_with_workspace(
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
    setup_agent(&app, &agent_id, &user).await?;

    // Write subscribed skill to disk
    let skill = bendclaw::kernel::skills::model::skill::Skill {
        name: "tool".to_string(),
        version: "1.0.0".to_string(),
        description: "alice tool".to_string(),
        scope: bendclaw::kernel::skills::model::skill::SkillScope::Shared,
        source: bendclaw::kernel::skills::model::skill::SkillSource::Agent,
        user_id: "alice".to_string(),
        created_by: Some("alice".to_string()),
        last_used_by: None,
        timeout: 30,
        executable: false,
        parameters: vec![],
        content: "# Alice Tool".to_string(),
        files: vec![],
        requires: None,
        manifest: None,
    };
    bendclaw::kernel::skills::sources::remote::writer::write_subscribed_skill(
        &workspace_root,
        &user,
        "alice",
        &skill,
    );

    // DELETE /v1/skills/alice/tool should trigger unsubscribe, not delete
    let del_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/v1/skills/alice/tool")
                .header("x-user-id", &user)
                .body(Body::empty())?,
        )
        .await?;
    assert_eq!(del_resp.status(), StatusCode::OK);

    // Verify unsubscribe was called (DELETE on resource_subscriptions)
    assert!(
        *unsub_called.lock().expect("lock"),
        "DELETE of namespaced skill should trigger unsubscribe"
    );

    Ok(())
}
