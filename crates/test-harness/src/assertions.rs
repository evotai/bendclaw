#![allow(dead_code)]

use anyhow::bail;
use anyhow::Context as _;
use anyhow::Result;
use serde_json::Value;

// ── Run assertions ────────────────────────────────────────────────────────────

/// Assert a chat response is successful and return the output string.
pub fn run_output(run: &Value) -> Result<&str> {
    if run["ok"] != true {
        bail!("expected run ok=true, got: {run}");
    }
    run["message"]
        .as_str()
        .context("run.message is not a string")
}

/// Assert run output equals expected string.
pub fn assert_output_eq(run: &Value, expected: &str) -> Result<()> {
    let msg = run_output(run)?;
    if msg != expected {
        bail!("run output mismatch: expected {expected:?}, got {msg:?}");
    }
    Ok(())
}

/// Assert run output contains a substring.
pub fn assert_output_contains(run: &Value, substr: &str) -> Result<()> {
    let msg = run_output(run)?;
    if !msg.contains(substr) {
        bail!("expected output to contain {substr:?}, got: {msg:?}");
    }
    Ok(())
}

/// Assert a runs list has exactly `count` entries.
pub fn assert_runs_count(runs: &[Value], count: usize) -> Result<()> {
    if runs.len() != count {
        bail!("expected {count} runs, got {}", runs.len());
    }
    Ok(())
}

/// Assert a specific run (by index) has the given input.
pub fn assert_run_input(runs: &[Value], idx: usize, expected: &str) -> Result<()> {
    let run = runs
        .get(idx)
        .with_context(|| format!("no run at index {idx}"))?;
    let input = run["input"]
        .as_str()
        .with_context(|| format!("run[{idx}].input is not a string"))?;
    if input != expected {
        bail!("run[{idx}].input mismatch: expected {expected:?}, got {input:?}");
    }
    Ok(())
}

// ── Event assertions ──────────────────────────────────────────────────────────

/// Assert that a named event is present in the events array.
pub fn assert_event_present(events: &[Value], name: &str) -> Result<()> {
    if !events.iter().any(|e| e["event"] == name) {
        bail!("missing event: {name}");
    }
    Ok(())
}

/// Assert that all named events are present.
pub fn assert_events_present(events: &[Value], names: &[&str]) -> Result<()> {
    for name in names {
        assert_event_present(events, name)?;
    }
    Ok(())
}

/// Assert that a tool was called (ToolStart event with matching name).
pub fn assert_tool_called(events: &[Value], tool_name: &str) -> Result<()> {
    let found = events.iter().any(|e| {
        e["event"] == "ToolStart" && e["payload"]["data"]["name"].as_str() == Some(tool_name)
    });
    if !found {
        bail!("expected ToolStart for tool {tool_name:?}");
    }
    Ok(())
}

/// Assert the count of ToolStart events equals `count`.
pub fn assert_tool_call_count(events: &[Value], count: usize) -> Result<()> {
    let actual = events.iter().filter(|e| e["event"] == "ToolStart").count();
    if actual != count {
        bail!("expected {count} ToolStart events, got {actual}");
    }
    Ok(())
}

// ── Session assertions ────────────────────────────────────────────────────────

/// Assert a session with the given ID exists in the list.
pub fn assert_session_exists(sessions: &[Value], session_id: &str) -> Result<()> {
    if !sessions
        .iter()
        .any(|s| s["id"].as_str() == Some(session_id))
    {
        bail!("session {session_id:?} not found in list");
    }
    Ok(())
}

/// Assert a session with the given ID does NOT exist in the list.
pub fn assert_session_not_exists(sessions: &[Value], session_id: &str) -> Result<()> {
    if sessions
        .iter()
        .any(|s| s["id"].as_str() == Some(session_id))
    {
        bail!("session {session_id:?} should not be in list");
    }
    Ok(())
}

/// Assert a session list has exactly `count` entries.
pub fn assert_sessions_count(sessions: &[Value], count: usize) -> Result<()> {
    if sessions.len() != count {
        bail!("expected {count} sessions, got {}", sessions.len());
    }
    Ok(())
}

// ── HTTP response assertions ──────────────────────────────────────────────────

/// Extract and return the data array from a JSON response body.
pub fn data_array(body: &Value) -> Result<&Vec<Value>> {
    body["data"]
        .as_array()
        .with_context(|| format!("expected body.data to be an array, got: {body}"))
}

/// Assert a JSON response body has an empty data array.
pub fn assert_data_empty(body: &Value) -> Result<()> {
    let arr = data_array(body)?;
    if !arr.is_empty() {
        bail!("expected empty data array, got {} items", arr.len());
    }
    Ok(())
}
