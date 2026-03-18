//! End-to-end integration tests: full agent flow with mock LLM and real Databend.

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;

use crate::common::api::TestApi;
use crate::common::assertions::assert_event_present;
use crate::common::assertions::assert_events_present;
use crate::common::assertions::assert_output_eq;
use crate::common::assertions::assert_runs_count;
use crate::common::assertions::assert_tool_call_count;
use crate::common::setup::uid;
use crate::common::setup::TestContext;
use crate::mocks::llm::MockLLMProvider;

#[tokio::test]
async fn e2e_tool_call_persists_events_and_structured_payloads() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let api = TestApi::new(
        ctx.app_with_llm(Arc::new(MockLLMProvider::from_fixture("tool_call_single")?))
            .await?,
    );
    let agent_id = uid("e2e-tc");
    let user = uid("user");
    let session_id = uid("session");

    api.setup_agent(&agent_id, &user).await?;
    let resp = api
        .chat(&agent_id, &session_id, &user, "run echo hello")
        .await?;
    assert_output_eq(&resp, "Command executed successfully.")?;

    let runs = api.get_runs(&agent_id, &session_id, &user).await?;
    assert_runs_count(&runs, 1)?;
    let run_id = runs[0]["id"].as_str().context("run id missing")?;
    assert_eq!(runs[0]["input"], "run echo hello");

    let detail = api.get_run_detail(&agent_id, run_id, &user).await?;
    let events = detail["events"].as_array().context("missing run events")?;
    assert_event_present(events, "ToolStart")?;
    assert_event_present(events, "ToolEnd")?;
    assert!(
        events.iter().any(|e| {
            e["event"] == "ToolStart"
                && e["payload"]["type"] == "ToolStart"
                && e["payload"]["data"]["tool_call_id"].is_string()
                && e["payload"]["data"]["arguments"].is_object()
        }),
        "missing structured ToolStart event"
    );
    assert!(
        events.iter().any(|e| {
            e["event"] == "ToolEnd"
                && e["payload"]["type"] == "ToolEnd"
                && e["payload"]["data"]["tool_call_id"].is_string()
                && e["payload"]["data"]["success"].is_boolean()
        }),
        "missing structured ToolEnd event"
    );
    Ok(())
}

#[tokio::test]
async fn e2e_phase1_audit_events_are_persisted() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let api = TestApi::new(
        ctx.app_with_llm(Arc::new(MockLLMProvider::from_fixture("audit_trail")?))
            .await?,
    );
    let agent_id = uid("e2e-phase1");
    let user = uid("user");
    let session_id = uid("session");

    api.setup_agent(&agent_id, &user).await?;
    api.chat(&agent_id, &session_id, &user, "show audit trail")
        .await?;

    let runs = api.get_runs(&agent_id, &session_id, &user).await?;
    let run_id = runs[0]["id"].as_str().context("run id missing")?;
    let detail = api.get_run_detail(&agent_id, run_id, &user).await?;
    let events = detail["events"].as_array().context("events missing")?;

    assert_events_present(events, &[
        "run.started",
        "prompt.built",
        "turn.started",
        "llm.request",
        "llm.response",
        "turn.completed",
        "run.completed",
    ])?;
    Ok(())
}

#[tokio::test]
async fn e2e_parallel_tool_calls_all_persisted() -> Result<()> {
    let ctx = TestContext::setup().await?;
    let api = TestApi::new(
        ctx.app_with_llm(Arc::new(MockLLMProvider::from_fixture(
            "tool_call_parallel",
        )?))
        .await?,
    );
    let agent_id = uid("e2e-par");
    let user = uid("user");
    let session_id = uid("session");

    api.setup_agent(&agent_id, &user).await?;
    let resp = api
        .chat(&agent_id, &session_id, &user, "run two commands")
        .await?;
    assert_output_eq(&resp, "Both commands done.")?;

    let runs = api.get_runs(&agent_id, &session_id, &user).await?;
    let run_id = runs[0]["id"].as_str().context("run id missing")?;
    let detail = api.get_run_detail(&agent_id, run_id, &user).await?;
    let events = detail["events"].as_array().context("events missing")?;

    assert_tool_call_count(events, 2)?;
    assert_eq!(events.iter().filter(|e| e["event"] == "ToolEnd").count(), 2);
    Ok(())
}

#[tokio::test]
async fn e2e_remote_skill_reads_variable_updates_last_used_and_emits_tool_output() -> Result<()> {
    let skill_name = uid("env-skill").to_lowercase();
    let llm = Arc::new(MockLLMProvider::new(vec![
        crate::mocks::llm::MockTurn::ToolCall {
            name: skill_name.clone(),
            arguments: "{}".into(),
        },
        crate::mocks::llm::MockTurn::Text("done".into()),
    ]));
    let ctx = TestContext::setup().await?;
    let api = TestApi::new(ctx.app_with_llm(llm).await?);
    let agent_id = uid("e2e-skill");
    let user = uid("user");
    let session_id = uid("session");

    api.setup_agent(&agent_id, &user).await?;
    let variable = api
        .create_variable(
            &agent_id,
            &user,
            serde_json::json!({
                "key": "API_TOKEN",
                "value": "live-secret",
                "secret": true,
            }),
        )
        .await?;
    let variable_id = variable["id"].as_str().context("variable id missing")?;
    let script = r#"#!/usr/bin/env bash
cat >/dev/null
printf '%s' "$API_TOKEN""#;
    api.create_skill(
        &agent_id,
        &user,
        serde_json::json!({
            "name": skill_name,
            "description": "reads API_TOKEN",
            "content": "Reads API_TOKEN and prints it.",
            "executable": true,
            "files": [{
                "path": "scripts/run.sh",
                "body": script,
            }],
            "requires": {
                "bins": ["bash"],
                "env": ["API_TOKEN"]
            }
        }),
    )
    .await?;

    let resp = api
        .chat(&agent_id, &session_id, &user, "run the env skill")
        .await?;
    assert_output_eq(&resp, "done")?;

    let runs = api.get_runs(&agent_id, &session_id, &user).await?;
    let run_id = runs[0]["id"].as_str().context("run id missing")?;
    let detail = api.get_run_detail(&agent_id, run_id, &user).await?;
    let events = detail["events"].as_array().context("events missing")?;

    assert!(
        events.iter().any(|e| {
            e["event"] == "ToolEnd"
                && e["payload"]["data"]["name"] == skill_name
                && e["payload"]["data"]["success"] == true
                && e["payload"]["data"]["output"] == "live-secret"
        }),
        "missing ToolEnd with remote skill output: {events:?}"
    );
    let variable = api.get_variable(&agent_id, &user, variable_id).await?;
    assert!(
        variable["last_used_at"].is_string(),
        "last_used_at not updated: {variable}"
    );
    Ok(())
}
