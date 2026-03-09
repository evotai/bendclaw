use super::http::CreateVariableRequest;
use super::http::UpdateVariableRequest;
use crate::base::new_id;
use crate::service::error::Result;
use crate::service::error::ServiceError;
use crate::service::state::AppState;
use crate::service::v1::common::count_u64;
use crate::service::v1::common::ListQuery;
use crate::storage::dal::variable::VariableRecord;
use crate::storage::dal::variable::VariableRepo;

pub(super) async fn list_variables(
    state: &AppState,
    agent_id: &str,
    q: &ListQuery,
) -> Result<(Vec<VariableRecord>, u64)> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = VariableRepo::new(pool.clone());
    let limit = q.limit();
    let records = repo.list(limit).await?;
    let total = count_u64(
        &pool,
        "SELECT COUNT(*) FROM variables",
    )
    .await;
    Ok((records, total))
}

pub(super) async fn create_variable(
    state: &AppState,
    agent_id: &str,
    req: CreateVariableRequest,
) -> Result<VariableRecord> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = VariableRepo::new(pool);
    let record = VariableRecord {
        id: new_id(),
        key: req.key,
        value: req.value,
        secret: req.secret.unwrap_or(false),
        created_at: String::new(),
        updated_at: String::new(),
    };
    repo.insert(&record).await?;
    Ok(record)
}

pub(super) async fn get_variable(
    state: &AppState,
    agent_id: &str,
    var_id: &str,
) -> Result<VariableRecord> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = VariableRepo::new(pool);
    repo.get(var_id)
        .await?
        .ok_or_else(|| ServiceError::AgentNotFound(format!("variable not found: {var_id}")))
}

pub(super) async fn update_variable(
    state: &AppState,
    agent_id: &str,
    var_id: &str,
    req: UpdateVariableRequest,
) -> Result<()> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = VariableRepo::new(pool);
    let existing = repo
        .get(var_id)
        .await?
        .ok_or_else(|| ServiceError::AgentNotFound(format!("variable not found: {var_id}")))?;
    let key = req.key.unwrap_or(existing.key);
    let value = req.value.unwrap_or(existing.value);
    let secret = req.secret.unwrap_or(existing.secret);
    repo.update(var_id, &key, &value, secret).await?;
    Ok(())
}

pub(super) async fn delete_variable(
    state: &AppState,
    agent_id: &str,
    var_id: &str,
) -> Result<()> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = VariableRepo::new(pool);
    repo.delete(var_id).await?;
    Ok(())
}
