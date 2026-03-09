use anyhow::Context;
use anyhow::Result;

use crate::common::setup::cleanup_prefix;
use crate::common::setup::require_api_config;
use crate::common::setup::uid;
use crate::mocks::llm::MockLLMProvider;

#[tokio::test]
async fn runtime_initial_status_ready() -> Result<()> {
    let (base_url, token, warehouse) = require_api_config()?;
    let llm = std::sync::Arc::new(MockLLMProvider::with_text("ok"));
    let prefix = format!("it_rt_{}_", uid("r"));
    let runtime = bendclaw::kernel::Runtime::new(
        &base_url,
        &token,
        &warehouse,
        &prefix,
        llm,
    )
    .build()
    .await?;

    assert_eq!(format!("{:?}", runtime.status()), "Ready");
    cleanup_prefix(&prefix).await?;
    Ok(())
}

#[tokio::test]
async fn runtime_shutdown_transitions_to_stopped() -> Result<()> {
    let (base_url, token, warehouse) = require_api_config()?;
    let llm = std::sync::Arc::new(MockLLMProvider::with_text("ok"));
    let prefix = format!("it_rt_{}_", uid("r"));
    let runtime = bendclaw::kernel::Runtime::new(
        &base_url,
        &token,
        &warehouse,
        &prefix,
        llm,
    )
    .build()
    .await?;

    runtime.shutdown().await?;
    assert_eq!(format!("{:?}", runtime.status()), "Stopped");
    cleanup_prefix(&prefix).await?;
    Ok(())
}

#[tokio::test]
async fn runtime_rejects_session_creation_after_shutdown() -> Result<()> {
    let (base_url, token, warehouse) = require_api_config()?;
    let llm = std::sync::Arc::new(MockLLMProvider::with_text("ok"));
    let prefix = format!("it_rt_{}_", uid("r"));
    let runtime = bendclaw::kernel::Runtime::new(
        &base_url,
        &token,
        &warehouse,
        &prefix,
        llm,
    )
    .build()
    .await?;

    let agent_id = uid("agent");
    runtime.setup_agent(&agent_id).await?;
    runtime.shutdown().await?;

    let err = runtime
        .get_or_create_session(&agent_id, &uid("session"), &uid("user"))
        .await
        .err()
        .context("expected session creation to fail after shutdown")?;
    assert_eq!(err.code, bendclaw::base::ErrorCode::INTERNAL);
    assert!(err.message.contains("runtime is not ready"));
    cleanup_prefix(&prefix).await?;
    Ok(())
}

#[tokio::test]
async fn runtime_setup_agent_is_idempotent() -> Result<()> {
    let (base_url, token, warehouse) = require_api_config()?;
    let llm = std::sync::Arc::new(MockLLMProvider::with_text("ok"));
    let prefix = format!("it_rt_{}_", uid("r"));
    let runtime = bendclaw::kernel::Runtime::new(
        &base_url,
        &token,
        &warehouse,
        &prefix,
        llm,
    )
    .build()
    .await?;

    let agent_id = uid("agent");
    runtime.setup_agent(&agent_id).await?;
    runtime.setup_agent(&agent_id).await?;

    let cfg = runtime.agent_config_store(&agent_id)?;
    let rec = cfg.get(&agent_id).await?;
    assert!(rec.is_some());
    cleanup_prefix(&prefix).await?;
    Ok(())
}
