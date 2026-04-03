use super::http::ConfigResponse;
use super::http::UpdateConfigRequest;
use super::http::VersionResponse;
use crate::server::error::Result;
use crate::server::error::ServiceError;
use crate::server::state::AppState;
use crate::server::v1::common::count_u64;
use crate::server::v1::common::ListQuery;
use crate::server::v1::common::Paginated;
use crate::storage::dal::config_version::record::ConfigVersionRecord;
use crate::storage::dal::config_version::repo::ConfigVersionRepo;

pub(super) async fn get_config(state: &AppState, agent_id: &str) -> Result<ConfigResponse> {
    let record = state.runtime.get_config(agent_id).await?;
    Ok(match record {
        Some(r) => ConfigResponse {
            agent_id: r.agent_id,
            system_prompt: r.system_prompt,
            identity: r.identity,
            soul: r.soul,
            token_limit_total: r.token_limit_total,
            token_limit_daily: r.token_limit_daily,
            llm_config: r.llm_config,
        },
        None => ConfigResponse {
            agent_id: agent_id.to_string(),
            system_prompt: String::new(),
            identity: String::new(),
            soul: String::new(),
            token_limit_total: None,
            token_limit_daily: None,
            llm_config: None,
        },
    })
}

pub(super) async fn update_config(
    state: &AppState,
    agent_id: &str,
    req: UpdateConfigRequest,
) -> Result<u32> {
    let llm_ref = req.llm_config.as_ref().map(|opt| opt.as_ref());
    let version = state
        .runtime
        .update_config_with_version(
            agent_id,
            req.system_prompt.as_deref(),
            req.identity.as_deref(),
            req.soul.as_deref(),
            req.token_limit_total,
            req.token_limit_daily,
            llm_ref,
            req.notes.as_deref(),
            req.label.as_deref(),
        )
        .await?;
    Ok(version)
}

pub(super) async fn rollback_config(
    state: &AppState,
    agent_id: &str,
    record: ConfigVersionRecord,
) -> Result<()> {
    state
        .runtime
        .upsert_config(
            agent_id,
            Some(&record.system_prompt),
            Some(&record.identity),
            Some(&record.soul),
            Some(record.token_limit_total),
            Some(record.token_limit_daily),
            Some(record.llm_config.as_ref()),
        )
        .await?;
    Ok(())
}

pub(super) async fn list_versions(
    state: &AppState,
    agent_id: &str,
    q: ListQuery,
) -> Result<Paginated<VersionResponse>> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = ConfigVersionRepo::new(pool.clone());
    let records = repo.list_by_agent(agent_id, q.limit()).await?;
    let aid = crate::storage::sql::escape(agent_id);
    let total = count_u64(
        &pool,
        &format!("SELECT COUNT(*) FROM agent_config_versions WHERE agent_id = '{aid}'"),
    )
    .await;
    Ok(Paginated::new(
        records.into_iter().map(to_version_response).collect(),
        &q,
        total,
    ))
}

pub(super) async fn get_version(
    state: &AppState,
    agent_id: &str,
    version: u32,
) -> Result<VersionResponse> {
    let record = load_version_record(state, agent_id, version).await?;
    Ok(to_version_response(record))
}

pub(super) async fn load_version_record(
    state: &AppState,
    agent_id: &str,
    version: u32,
) -> Result<ConfigVersionRecord> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = ConfigVersionRepo::new(pool);
    repo.get_version(agent_id, version)
        .await?
        .ok_or_else(|| ServiceError::AgentNotFound(format!("version {version} not found")))
}

fn to_version_response(r: ConfigVersionRecord) -> VersionResponse {
    VersionResponse {
        id: r.id,
        version: r.version,
        label: r.label,
        stage: r.stage,
        system_prompt: r.system_prompt,
        identity: r.identity,
        soul: r.soul,
        token_limit_total: r.token_limit_total,
        token_limit_daily: r.token_limit_daily,
        llm_config: r.llm_config,
        notes: r.notes,
        created_at: r.created_at,
    }
}
