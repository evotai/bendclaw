use anyhow::bail;
use anyhow::Result;
use bendclaw::base::OptionExt;
use bendclaw::base::ResultExt;
use bendclaw::kernel::ErrorCode;

#[test]
fn error_code_internal() {
    let e = ErrorCode::internal("something broke");
    assert_eq!(e.code, ErrorCode::INTERNAL);
    assert_eq!(e.name, "Internal");
    assert!(e.message.contains("something broke"));
}

#[test]
fn error_code_not_found() {
    let e = ErrorCode::not_found("missing");
    assert_eq!(e.code, ErrorCode::NOT_FOUND);
}

#[test]
fn error_code_timeout() {
    let e = ErrorCode::timeout("too slow");
    assert_eq!(e.code, ErrorCode::TIMEOUT);
}

#[test]
fn error_code_with_context() {
    let e = ErrorCode::internal("base").with_context(|| "extra context".into());
    assert_eq!(e.stacks.len(), 1);
    let display = e.to_string();
    assert!(display.contains("extra context"));
}

#[test]
fn error_code_add_message() {
    let e = ErrorCode::internal("original").add_message("prefix");
    assert!(e.message.starts_with("prefix:"));
}

#[test]
fn error_code_add_message_back() {
    let e = ErrorCode::internal("original").add_message_back("suffix");
    assert!(e.message.ends_with("suffix"));
}

#[test]
fn error_code_http_status_not_found() {
    let e = ErrorCode::not_found("x");
    assert_eq!(e.http_status(), 404);
}

#[test]
fn error_code_http_status_auth() {
    assert_eq!(ErrorCode::auth_request("x").http_status(), 401);
    assert_eq!(ErrorCode::auth_credentials("x").http_status(), 401);
    assert_eq!(ErrorCode::auth_token_expired("x").http_status(), 401);
    assert_eq!(ErrorCode::auth_parse("x").http_status(), 401);
}

#[test]
fn error_code_http_status_denied() {
    assert_eq!(ErrorCode::denied("x").http_status(), 403);
}

#[test]
fn error_code_http_status_quota() {
    assert_eq!(ErrorCode::quota_exceeded("x").http_status(), 429);
}

#[test]
fn error_code_http_status_timeout() {
    assert_eq!(ErrorCode::timeout("x").http_status(), 408);
    assert_eq!(ErrorCode::skill_timeout("x").http_status(), 408);
}

#[test]
fn error_code_http_status_internal_default() {
    assert_eq!(ErrorCode::internal("x").http_status(), 500);
    assert_eq!(ErrorCode::storage_exec("x").http_status(), 500);
    assert_eq!(ErrorCode::llm_request("x").http_status(), 500);
}

#[test]
fn error_code_display() {
    let e = ErrorCode::internal("test msg");
    let s = e.to_string();
    assert!(s.contains("[1001]"));
    assert!(s.contains("Internal"));
    assert!(s.contains("test msg"));
}

#[test]
fn error_code_from_serde_json() -> Result<()> {
    let Err(err) = serde_json::from_str::<String>("invalid") else {
        bail!("expected serde_json error");
    };
    let ec: ErrorCode = err.into();
    assert_eq!(ec.code, ErrorCode::INTERNAL);
    Ok(())
}

#[test]
fn error_code_from_io_error() {
    let err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
    let ec: ErrorCode = err.into();
    assert_eq!(ec.code, ErrorCode::INTERNAL);
}

#[test]
fn error_code_all_storage_variants() {
    assert_eq!(
        ErrorCode::storage_connection("x").code,
        ErrorCode::STORAGE_CONNECTION
    );
    assert_eq!(ErrorCode::storage_exec("x").code, ErrorCode::STORAGE_EXEC);
    assert_eq!(ErrorCode::storage_query("x").code, ErrorCode::STORAGE_QUERY);
    assert_eq!(
        ErrorCode::storage_migration("x").code,
        ErrorCode::STORAGE_MIGRATION
    );
    assert_eq!(ErrorCode::storage_serde("x").code, ErrorCode::STORAGE_SERDE);
}

#[test]
fn error_code_all_llm_variants() {
    assert_eq!(ErrorCode::llm_request("x").code, ErrorCode::LLM_REQUEST);
    assert_eq!(ErrorCode::llm_response("x").code, ErrorCode::LLM_RESPONSE);
    assert_eq!(
        ErrorCode::llm_rate_limit("x").code,
        ErrorCode::LLM_RATE_LIMIT
    );
    assert_eq!(ErrorCode::llm_server("x").code, ErrorCode::LLM_SERVER);
    assert_eq!(ErrorCode::llm_parse("x").code, ErrorCode::LLM_PARSE);
}

#[test]
fn error_code_all_skill_variants() {
    assert_eq!(
        ErrorCode::skill_not_found("x").code,
        ErrorCode::SKILL_NOT_FOUND
    );
    assert_eq!(ErrorCode::skill_exec("x").code, ErrorCode::SKILL_EXEC);
    assert_eq!(ErrorCode::skill_timeout("x").code, ErrorCode::SKILL_TIMEOUT);
    assert_eq!(ErrorCode::skill_serde("x").code, ErrorCode::SKILL_SERDE);
    assert_eq!(
        ErrorCode::skill_validation("x").code,
        ErrorCode::SKILL_VALIDATION
    );
    assert_eq!(
        ErrorCode::skill_requirements("x").code,
        ErrorCode::SKILL_REQUIREMENTS
    );
}

#[test]
fn error_code_sandbox() {
    assert_eq!(ErrorCode::sandbox("x").code, ErrorCode::SANDBOX);
}

#[test]
fn error_code_config() {
    assert_eq!(ErrorCode::config("x").code, ErrorCode::CONFIG);
}

#[test]
fn error_code_invalid_input() {
    assert_eq!(ErrorCode::invalid_input("x").code, ErrorCode::INVALID_INPUT);
}

#[test]
fn result_ext_with_context_ok() -> Result<()> {
    let r: std::result::Result<i32, ErrorCode> = Ok(42);
    let val = r
        .with_context(|| "ctx".into())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    assert_eq!(val, 42);
    Ok(())
}

#[test]
fn result_ext_with_context_err() -> Result<()> {
    let r: std::result::Result<i32, ErrorCode> = Err(ErrorCode::internal("base"));
    let Err(e) = r.with_context(|| "added context".into()) else {
        bail!("expected error");
    };
    assert!(!e.stacks.is_empty());
    Ok(())
}

#[test]
fn option_ext_ok_or_not_found_some() -> Result<()> {
    let o: Option<i32> = Some(42);
    let val = o
        .ok_or_not_found(|| "missing".into())
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    assert_eq!(val, 42);
    Ok(())
}

#[test]
fn option_ext_ok_or_not_found_none() -> Result<()> {
    let o: Option<i32> = None;
    let Err(e) = o.ok_or_not_found(|| "missing".into()) else {
        bail!("expected error");
    };
    assert_eq!(e.code, ErrorCode::NOT_FOUND);
    Ok(())
}

#[test]
fn option_ext_ok_or_error_some() -> Result<()> {
    let o: Option<i32> = Some(1);
    let val = o
        .ok_or_error(|| ErrorCode::internal("x"))
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    assert_eq!(val, 1);
    Ok(())
}

#[test]
fn option_ext_ok_or_error_none() -> Result<()> {
    let o: Option<i32> = None;
    let Err(e) = o.ok_or_error(|| ErrorCode::timeout("slow")) else {
        bail!("expected error");
    };
    assert_eq!(e.code, ErrorCode::TIMEOUT);
    Ok(())
}
