//! Tests for VariableRecord serde and field semantics.

use anyhow::Result;
use bendclaw::storage::dal::variable::VariableRecord;

fn make_var(key: &str, value: &str, secret: bool) -> VariableRecord {
    VariableRecord {
        id: "var-001".into(),
        key: key.to_string(),
        value: value.to_string(),
        secret,
        last_used_at: None,
        created_at: "2026-01-01T00:00:00Z".into(),
        updated_at: "2026-01-01T00:00:00Z".into(),
    }
}

#[test]
fn variable_record_serde_roundtrip() -> Result<()> {
    let rec = make_var("API_KEY", "secret-value", true);
    let json = serde_json::to_string(&rec)?;
    let back: VariableRecord = serde_json::from_str(&json)?;
    assert_eq!(back.id, "var-001");
    assert_eq!(back.key, "API_KEY");
    assert_eq!(back.value, "secret-value");
    assert!(back.secret);
    assert!(back.last_used_at.is_none());
    Ok(())
}

#[test]
fn variable_record_non_secret_serde() -> Result<()> {
    let rec = make_var("REGION", "us-east-1", false);
    let json = serde_json::to_string(&rec)?;
    let back: VariableRecord = serde_json::from_str(&json)?;
    assert_eq!(back.key, "REGION");
    assert_eq!(back.value, "us-east-1");
    assert!(!back.secret);
    Ok(())
}

#[test]
fn variable_record_with_last_used_at() -> Result<()> {
    let mut rec = make_var("TOKEN", "abc", false);
    rec.last_used_at = Some("2026-06-01T12:00:00Z".into());
    let json = serde_json::to_string(&rec)?;
    let back: VariableRecord = serde_json::from_str(&json)?;
    assert_eq!(back.last_used_at.as_deref(), Some("2026-06-01T12:00:00Z"));
    Ok(())
}

// ── Secret masking semantics ──
//
// The HTTP layer masks secret values as "****" in responses.
// The prompt layer shows "[SECRET]" for secret variables.
// These tests verify the raw record preserves the actual value
// (masking is the responsibility of the presentation layer).

#[test]
fn variable_record_secret_preserves_raw_value() {
    let rec = make_var("DB_PASSWORD", "hunter2", true);
    // Raw record always stores the real value
    assert_eq!(rec.value, "hunter2");
    assert!(rec.secret);
}

#[test]
fn variable_record_non_secret_value_is_plaintext() {
    let rec = make_var("LOG_LEVEL", "debug", false);
    assert_eq!(rec.value, "debug");
    assert!(!rec.secret);
}
