use evot::auth;
use napi::Result as NapiResult;
use napi_derive::napi;

fn to_json<T: serde::Serialize>(value: &T) -> NapiResult<String> {
    serde_json::to_string(value)
        .map_err(|error| napi::Error::new(napi::Status::GenericFailure, error.to_string()))
}

#[napi]
pub async fn auth_begin(server_url: String, fingerprint_id: String) -> NapiResult<String> {
    let response = auth::begin_login(&server_url, &fingerprint_id)
        .await
        .map_err(to_napi)?;
    to_json(&response)
}

#[napi]
pub async fn auth_poll(server_url: String, code: String, expires_at: i64) -> NapiResult<String> {
    match auth::poll_status(&server_url, &code, expires_at).await {
        Ok(auth::PollOutcome::Pending) => Ok(r#"{"status":"pending"}"#.to_string()),
        Ok(auth::PollOutcome::Expired) => Ok(r#"{"status":"expired"}"#.to_string()),
        Ok(auth::PollOutcome::Denied) => Ok(r#"{"status":"denied"}"#.to_string()),
        Ok(auth::PollOutcome::Success { user }) => {
            auth::save_auth(&user).map_err(to_napi)?;
            match sync_models_inner().await {
                Ok(cache) => to_json(
                    &serde_json::json!({ "status": "success", "state": user, "models": cache.response }),
                ),
                Err(error) => to_json(
                    &serde_json::json!({ "status": "success", "state": user, "sync_error": error.to_string() }),
                ),
            }
        }
        Err(error) => Err(to_napi(error)),
    }
}

#[napi]
pub async fn auth_sync_models() -> NapiResult<String> {
    let cache = sync_models_inner().await.map_err(to_napi)?;
    to_json(&cache.response)
}

async fn sync_models_inner() -> evot::error::Result<evot::auth::ModelsCache> {
    let state =
        auth::load_auth()?.ok_or_else(|| evot::error::EvotError::Conf("not logged in".into()))?;
    let response = auth::sync_models(&state).await?;
    let cache = evot::auth::ModelsCache {
        synced_at: now_ms(),
        response,
    };
    auth::save_models_cache(&cache)?;
    Ok(cache)
}

#[napi]
pub fn auth_logout() -> NapiResult<()> {
    auth::logout().map_err(to_napi)
}

#[napi]
pub fn auth_whoami() -> Option<String> {
    let state = auth::load_auth().ok()??;
    serde_json::to_string(&state.user).ok()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn to_napi<E: std::fmt::Display>(error: E) -> napi::Error {
    napi::Error::new(napi::Status::GenericFailure, error.to_string())
}

use std::collections::HashMap;

pub fn cloud_model_meta() -> HashMap<String, evot::auth::FreeModelOption> {
    let mut map = HashMap::new();
    if let Ok(Some(cache)) = evot::auth::load_models_cache() {
        for model in cache.response.models {
            map.insert(model.id.clone(), model);
        }
    }
    map
}

/// Presentation for each server-pushed provider group, keyed by provider name.
/// The server owns the heading and the ordering; the CLI only renders them.
pub fn cloud_provider_groups() -> HashMap<String, (String, i64)> {
    let mut map = HashMap::new();
    if let Ok(Some(cache)) = evot::auth::load_models_cache() {
        for group in cache.response.providers {
            let name = group.name.clone();
            map.insert(name, (group.label, group.sort_order));
        }
    }
    map
}

#[napi]
pub fn auth_notices() -> Option<String> {
    let cache = evot::auth::load_models_cache().ok()??;
    serde_json::to_string(&cache.response.notices).ok()
}
