#![allow(dead_code)]

use std::sync::Arc;
use std::sync::Mutex;

use super::fake_databend::paged_rows;
use super::fake_databend::FakeDatabend;

#[derive(Clone, Default)]
pub struct RunExecState {
    pub run: Arc<Mutex<Option<StoredRun>>>,
}

#[derive(Clone)]
pub struct StoredRun {
    pub id: String,
    pub session_id: String,
    pub status: String,
    pub input: String,
    pub output: String,
    pub error: String,
    pub metrics: String,
    pub stop_reason: String,
    pub iterations: u32,
}

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

pub fn stored_run_row(run: &StoredRun) -> bendclaw::storage::pool::QueryResponse {
    bendclaw::storage::pool::QueryResponse {
        id: String::new(),
        state: "Succeeded".to_string(),
        error: None,
        data: vec![vec![
            serde_json::Value::String(run.id.clone()),
            serde_json::Value::String(run.session_id.clone()),
            serde_json::Value::String("agent-1".to_string()),
            serde_json::Value::String("user-1".to_string()),
            serde_json::Value::String(String::new()),
            serde_json::Value::String(String::new()),
            serde_json::Value::String(run.status.clone()),
            serde_json::Value::String(run.input.clone()),
            serde_json::Value::String(run.output.clone()),
            serde_json::Value::String(run.error.clone()),
            serde_json::Value::String(run.metrics.clone()),
            serde_json::Value::String(run.stop_reason.clone()),
            serde_json::Value::String(run.iterations.to_string()),
            serde_json::Value::String("2026-03-11T00:00:00Z".to_string()),
            serde_json::Value::String("2026-03-11T00:01:00Z".to_string()),
        ]],
        next_uri: None,
        final_uri: None,
        schema: Vec::new(),
    }
}

/// Build a FakeDatabend that handles the standard run-execution SQL patterns.
pub fn fake_run_exec_databend(state: RunExecState, session_id: &str) -> FakeDatabend {
    let session_id = session_id.to_string();
    let fake_state = state;
    FakeDatabend::new(move |sql, _database| {
        if sql.starts_with("SHOW DATABASES LIKE ") {
            return Ok(paged_rows(&[], None, None));
        }
        if sql.starts_with("SELECT agent_id, system_prompt, display_name, description, identity, soul, token_limit_total, token_limit_daily, llm_config, TO_VARCHAR(created_at), TO_VARCHAR(updated_at) FROM agent_config WHERE agent_id = ") {
            return Ok(paged_rows(&[], None, None));
        }
        if sql.starts_with("SELECT id, key, value, secret, revoked, TO_VARCHAR(last_used_at), TO_VARCHAR(created_at), TO_VARCHAR(updated_at) FROM variables WHERE revoked = FALSE") {
            return Ok(paged_rows(&[], None, None));
        }
        if sql.starts_with("SELECT id, agent_id, user_id, title, scope, base_key, replaced_by_session_id, reset_reason, PARSE_JSON(session_state), PARSE_JSON(meta), TO_VARCHAR(created_at), TO_VARCHAR(updated_at) FROM sessions WHERE id = ") {
            return Ok(paged_rows(&[], None, None));
        }
        if sql.starts_with("REPLACE INTO sessions ") {
            return Ok(paged_rows(&[], None, None));
        }
        if sql.contains(&format!(
            "WHERE session_id = '{session_id}' ORDER BY created_at DESC"
        )) {
            let guard = fake_state.run.lock().expect("run state");
            return Ok(match guard.as_ref() {
                Some(run) => stored_run_row(run),
                None => paged_rows(&[], None, None),
            });
        }
        if sql.starts_with("INSERT INTO runs ") {
            let values = quoted_values(sql);
            *fake_state.run.lock().expect("run state") = Some(StoredRun {
                id: values[0].clone(),
                session_id: values[1].clone(),
                status: values[6].clone(),
                input: values[7].clone(),
                output: values[8].clone(),
                error: values[9].clone(),
                metrics: values[10].clone(),
                stop_reason: values[11].clone(),
                iterations: 0,
            });
            return Ok(paged_rows(&[], None, None));
        }
        if sql.starts_with("INSERT INTO traces ")
            || sql.starts_with("UPDATE traces SET ")
            || sql.starts_with("INSERT INTO spans ")
            || sql.starts_with("INSERT INTO run_events ")
            || sql.starts_with("INSERT INTO usage ")
        {
            return Ok(paged_rows(&[], None, None));
        }
        if sql.contains("FROM spans") && sql.contains("status = 'failed'") {
            return Ok(paged_rows(&[], None, None));
        }
        if sql.starts_with("UPDATE runs SET status = ") {
            let values = quoted_values(sql);
            let run_id = values.last().cloned().unwrap_or_default();
            let mut guard = fake_state.run.lock().expect("run state");
            if let Some(run) = guard.as_mut() {
                if run.id == run_id {
                    run.status = values[0].clone();
                    if values.len() >= 6 {
                        run.output = values[1].clone();
                        run.error = values[2].clone();
                        run.metrics = values[3].clone();
                        run.stop_reason = values[4].clone();
                        run.iterations = sql
                            .split("iterations = ")
                            .nth(1)
                            .and_then(|rest| rest.split(',').next())
                            .and_then(|n| n.trim().parse::<u32>().ok())
                            .unwrap_or(run.iterations);
                    }
                }
            }
            return Ok(paged_rows(&[], None, None));
        }
        if sql.starts_with("SELECT id, session_id, agent_id, user_id, parent_run_id, node_id, status, input, output, error, metrics, stop_reason, iterations, TO_VARCHAR(created_at), TO_VARCHAR(updated_at) FROM runs WHERE id = ") {
            let guard = fake_state.run.lock().expect("run state");
            return Ok(match guard.as_ref() {
                Some(run) => stored_run_row(run),
                None => paged_rows(&[], None, None),
            });
        }
        if sql.starts_with("SELECT id, run_id, session_id, agent_id, user_id, seq, event, payload, TO_VARCHAR(created_at) FROM run_events WHERE run_id = ") {
            return Ok(paged_rows(&[], None, None));
        }
        panic!("unexpected SQL in fake run exec: {sql}");
    })
}
