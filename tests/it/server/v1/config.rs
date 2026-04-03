use std::sync::Arc;

use anyhow::Result;
use axum::body::Body;
use axum::http::Request;
use axum::http::StatusCode;
use tower::ServiceExt;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;
use crate::common::setup::app_with_root_pool_and_llm;
use crate::common::setup::json_body;
use crate::mocks::llm::MockLLMProvider;

#[tokio::test]
async fn put_config_rejects_invalid_llm_config() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        if sql.starts_with("CREATE ") || sql.starts_with("--") {
            return Ok(paged_rows(&[], None, None));
        }
        if sql.contains("evotai_meta.evotai_agents") {
            return Ok(paged_rows(&[], None, None));
        }
        panic!("unexpected SQL for invalid llm_config request: {sql}");
    });
    let prefix = format!(
        "test_invalid_llm_config_{}_",
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

    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/v1/agents/agent-1/config")
                .header("content-type", "application/json")
                .header("x-user-id", "user-1")
                .body(Body::from(
                    serde_json::json!({
                        "llm_config": {
                            "providers": []
                        }
                    })
                    .to_string(),
                ))?,
        )
        .await?;

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = json_body(resp).await?;
    assert!(body["error"]["message"]
        .as_str()
        .is_some_and(|error| error.contains("llm_config.providers must not be empty")));
    Ok(())
}
