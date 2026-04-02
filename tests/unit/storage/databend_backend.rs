use bendclaw::storage::backend::agent_repo::AgentRepo;
use bendclaw::storage::backend::databend::DatabendBackend;
use bendclaw::storage::backend::kind::StorageKind;
use bendclaw::storage::backend::storage_backend::StorageBackend;

#[test]
fn databend_backend_kind_is_cloud() {
    let pool = bendclaw::storage::pool::Pool::noop();
    let backend = DatabendBackend::new(pool);
    assert_eq!(backend.kind(), StorageKind::Cloud);
}

#[tokio::test]
async fn databend_stubs_return_errors() {
    let pool = bendclaw::storage::pool::Pool::noop();
    let backend = DatabendBackend::new(pool);

    let result = backend.get_agent("u01", "a01").await;
    assert!(result.is_err());
    let err_msg = format!("{}", result.unwrap_err());
    assert!(err_msg.contains("cloud mapping pending"));
}
