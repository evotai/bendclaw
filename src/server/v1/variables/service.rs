use super::http::CreateVariableRequest;
use super::http::UpdateVariableRequest;
use crate::server::error::Result;
use crate::server::error::ServiceError;
use crate::server::state::AppState;
use crate::types::new_id;
use crate::variables::store::Variable;
use crate::variables::store::VariableScope;

pub(super) async fn list_variables(state: &AppState, user_id: &str) -> Result<Vec<Variable>> {
    let vars = state.runtime.org().variables().list_all(user_id).await?;
    Ok(vars)
}

pub(super) async fn create_variable(
    state: &AppState,
    user_id: &str,
    req: CreateVariableRequest,
) -> Result<Variable> {
    let variable = Variable {
        id: new_id(),
        key: req.key,
        value: req.value,
        secret: req.secret.unwrap_or(false),
        revoked: req.revoked.unwrap_or(false),
        user_id: user_id.to_string(),
        scope: VariableScope::Shared,
        created_by: user_id.to_string(),
        last_used_at: None,
        last_used_by: None,
        created_at: String::new(),
        updated_at: String::new(),
    };
    state
        .runtime
        .org()
        .variables()
        .create(variable.clone())
        .await?;
    Ok(variable)
}

pub(super) async fn get_variable(
    state: &AppState,
    user_id: &str,
    var_id: &str,
) -> Result<Variable> {
    state
        .runtime
        .org()
        .variables()
        .get(user_id, var_id)
        .await?
        .ok_or_else(|| ServiceError::AgentNotFound(format!("variable not found: {var_id}")))
}

pub(super) async fn update_variable(
    state: &AppState,
    user_id: &str,
    var_id: &str,
    req: UpdateVariableRequest,
) -> Result<()> {
    let existing = state
        .runtime
        .org()
        .variables()
        .get(user_id, var_id)
        .await?
        .ok_or_else(|| ServiceError::AgentNotFound(format!("variable not found: {var_id}")))?;
    let key = req.key.unwrap_or(existing.key);
    let value = req.value.unwrap_or(existing.value);
    let secret = req.secret.unwrap_or(existing.secret);
    let revoked = req.revoked.unwrap_or(existing.revoked);
    state
        .runtime
        .org()
        .variables()
        .update(user_id, var_id, &key, &value, secret, revoked)
        .await?;
    Ok(())
}

pub(super) async fn delete_variable(state: &AppState, user_id: &str, var_id: &str) -> Result<()> {
    state
        .runtime
        .org()
        .variables()
        .delete(user_id, var_id)
        .await?;
    Ok(())
}
