//! Tests for NoopBackend: context provider, run initializer.

#[tokio::test]
async fn noop_backend_load_history_returns_empty() {
    use bendclaw::sessions::backend::context::SessionContextProvider;
    use bendclaw::sessions::backend::noop::NoopBackend;

    let backend = NoopBackend;
    let history = backend.load_history(100).await.unwrap();
    assert!(history.is_empty());
}

#[tokio::test]
async fn noop_backend_enforce_token_limits_succeeds() {
    use bendclaw::sessions::backend::context::SessionContextProvider;
    use bendclaw::sessions::backend::noop::NoopBackend;

    let backend = NoopBackend;
    assert!(backend.enforce_token_limits().await.is_ok());
}

#[test]
fn noop_backend_init_run_returns_run_id() {
    use bendclaw::sessions::backend::noop::NoopBackend;
    use bendclaw::sessions::backend::sink::RunInitializer;

    let backend = NoopBackend;
    let run_id = backend.init_run("hello", None, "node-1").unwrap();
    assert!(!run_id.is_empty());
}
