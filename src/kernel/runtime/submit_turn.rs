use std::sync::Arc;
use std::time::Instant;

use crate::base::Result;
use crate::kernel::agent_store::AgentStore;
use crate::kernel::runtime::diagnostics;
use crate::kernel::runtime::Runtime;
use crate::kernel::skills::skill::Skill;
use crate::llm::config::LLMConfig;
use crate::observability::redaction;
use crate::storage::dal::session::record::SessionRecord;

/// Validate that an LLMConfig can actually produce a working LLMRouter.
/// Called before persisting to avoid storing broken configs.
fn validate_llm_config(cfg: &LLMConfig) -> Result<()> {
    cfg.validate()
}

fn runtime_payload(payload: serde_json::Value) -> String {
    serde_json::to_string(&redaction::redact(payload)).unwrap_or_else(|_| "{}".to_string())
}

fn log_runtime_info(
    command: &str,
    status: &str,
    agent_id: &str,
    elapsed_ms: u64,
    payload: serde_json::Value,
) {
    let payload = runtime_payload(payload);
    diagnostics::log_runtime_command_completed(command, status, agent_id, elapsed_ms, &payload);
}

fn log_runtime_error(
    command: &str,
    agent_id: &str,
    elapsed_ms: u64,
    error: &impl std::fmt::Display,
    payload: serde_json::Value,
) {
    let payload = runtime_payload(payload);
    diagnostics::log_runtime_command_failed(command, agent_id, elapsed_ms, error, &payload);
}

impl Runtime {
    fn agent_store(&self, agent_id: &str) -> Result<Arc<AgentStore>> {
        self.require_ready()?;
        let pool = self.databases.agent_pool(agent_id)?;
        Ok(Arc::new(AgentStore::new(pool, self.llm.read().clone())))
    }

    pub async fn delete_session(&self, agent_id: &str, session_id: &str) -> Result<()> {
        let started = Instant::now();
        let had_live_session = self.sessions.get(session_id).is_some();
        let payload = serde_json::json!({
            "session_id": session_id,
            "had_live_session": had_live_session,
        });
        log_runtime_info("delete_session", "started", agent_id, 0, payload.clone());
        let result = self
            .session_lifecycle()
            .delete_session(agent_id, session_id)
            .await;
        match &result {
            Ok(_) => log_runtime_info(
                "delete_session",
                "completed",
                agent_id,
                started.elapsed().as_millis() as u64,
                payload,
            ),
            Err(error) => log_runtime_error(
                "delete_session",
                agent_id,
                started.elapsed().as_millis() as u64,
                error,
                payload,
            ),
        }
        result
    }

    pub async fn cancel_run(&self, agent_id: &str, run_id: &str) -> Result<()> {
        let started = Instant::now();
        let payload = serde_json::json!({"run_id": run_id});
        log_runtime_info("cancel_run", "started", agent_id, 0, payload.clone());
        self.require_ready()?;
        let pool = self.databases.agent_pool(agent_id)?;
        let repo = crate::storage::dal::run::repo::RunRepo::new(pool);

        let result = async {
            if let Some(record) = repo.load(run_id).await? {
                if let Some(session) = self.sessions.get(&record.session_id) {
                    let _ = session.cancel_run(run_id);
                }
                if record.status == crate::storage::dal::run::RunStatus::Pending.as_str() {
                    repo.update_status(run_id, crate::storage::dal::run::RunStatus::Cancelled)
                        .await?;
                }
            }
            Ok(())
        }
        .await;

        match &result {
            Ok(_) => log_runtime_info(
                "cancel_run",
                "completed",
                agent_id,
                started.elapsed().as_millis() as u64,
                payload,
            ),
            Err(error) => log_runtime_error(
                "cancel_run",
                agent_id,
                started.elapsed().as_millis() as u64,
                error,
                payload,
            ),
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_config_with_version(
        &self,
        agent_id: &str,
        system_prompt: Option<&str>,
        identity: Option<&str>,
        soul: Option<&str>,
        token_limit_total: Option<Option<u64>>,
        token_limit_daily: Option<Option<u64>>,
        llm_config: Option<Option<&LLMConfig>>,
        notes: Option<&str>,
        label: Option<&str>,
    ) -> Result<u32> {
        let started = Instant::now();
        let payload = serde_json::json!({
            "system_prompt": system_prompt,
            "identity": identity,
            "soul": soul,
            "token_limit_total": token_limit_total,
            "token_limit_daily": token_limit_daily,
            "llm_config": llm_config.is_some(),
            "notes": notes,
            "label": label,
        });
        log_runtime_info(
            "update_config_with_version",
            "started",
            agent_id,
            0,
            payload.clone(),
        );
        if let Some(Some(cfg)) = llm_config {
            validate_llm_config(cfg)?;
        }
        let result = self
            .agent_store(agent_id)?
            .config_update_with_version(
                agent_id,
                system_prompt,
                identity,
                soul,
                token_limit_total,
                token_limit_daily,
                llm_config,
                notes,
                label,
            )
            .await;
        match &result {
            Ok(version) => {
                self.invalidate_agent_llm(agent_id);
                log_runtime_info(
                    "update_config_with_version",
                    "completed",
                    agent_id,
                    started.elapsed().as_millis() as u64,
                    serde_json::json!({"payload": payload, "version": version}),
                );
            }
            Err(error) => log_runtime_error(
                "update_config_with_version",
                agent_id,
                started.elapsed().as_millis() as u64,
                error,
                payload,
            ),
        }
        result
    }

    pub async fn create_skill(&self, user_id: &str, skill: Skill) -> Result<()> {
        let started = Instant::now();
        let payload = serde_json::json!({
            "name": skill.name.clone(),
            "version": skill.version.clone(),
            "scope": format!("{:?}", skill.scope),
            "source": format!("{:?}", skill.source),
            "description": skill.description.clone(),
            "content": skill.content.clone(),
            "executable": skill.executable,
            "timeout": skill.timeout,
            "parameters": skill.parameters.clone(),
            "files": skill.files.clone(),
            "requires": skill.requires.clone(),
        });
        log_runtime_info("create_skill", "started", user_id, 0, payload.clone());
        self.require_ready()?;
        let result = self.org.skills().create(user_id, skill).await;
        match &result {
            Ok(_) => log_runtime_info(
                "create_skill",
                "completed",
                user_id,
                started.elapsed().as_millis() as u64,
                payload,
            ),
            Err(error) => log_runtime_error(
                "create_skill",
                user_id,
                started.elapsed().as_millis() as u64,
                error,
                payload,
            ),
        }
        result
    }

    pub async fn delete_skill(&self, user_id: &str, skill_name: &str) -> Result<()> {
        let started = Instant::now();
        let payload = serde_json::json!({
            "skill_name": skill_name,
        });
        log_runtime_info("delete_skill", "started", user_id, 0, payload.clone());
        self.require_ready()?;
        let result = self.org.skills().delete(user_id, skill_name).await;
        match &result {
            Ok(_) => log_runtime_info(
                "delete_skill",
                "completed",
                user_id,
                started.elapsed().as_millis() as u64,
                payload,
            ),
            Err(error) => log_runtime_error(
                "delete_skill",
                user_id,
                started.elapsed().as_millis() as u64,
                error,
                payload,
            ),
        }
        result
    }

    pub async fn upsert_session(
        &self,
        agent_id: &str,
        session_id: &str,
        user_id: &str,
        title: Option<&str>,
        session_state: Option<&serde_json::Value>,
        meta: Option<&serde_json::Value>,
    ) -> Result<SessionRecord> {
        let started = Instant::now();
        let payload = serde_json::json!({
            "session_id": session_id,
            "user_id": user_id,
            "title": title,
            "session_state": session_state,
            "meta": meta,
        });
        log_runtime_info("upsert_session", "started", agent_id, 0, payload.clone());
        let result = async {
            self.session_lifecycle()
                .update_session(agent_id, session_id, user_id, title, session_state, meta)
                .await
        }
        .await;
        match &result {
            Ok(_) => log_runtime_info(
                "upsert_session",
                "completed",
                agent_id,
                started.elapsed().as_millis() as u64,
                payload,
            ),
            Err(error) => log_runtime_error(
                "upsert_session",
                agent_id,
                started.elapsed().as_millis() as u64,
                error,
                payload,
            ),
        }
        result
    }

    pub async fn get_config(
        &self,
        agent_id: &str,
    ) -> Result<Option<crate::storage::dal::agent_config::record::AgentConfigRecord>> {
        self.agent_store(agent_id)?.config_get(agent_id).await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn upsert_config(
        &self,
        agent_id: &str,
        system_prompt: Option<&str>,
        identity: Option<&str>,
        soul: Option<&str>,
        token_limit_total: Option<Option<u64>>,
        token_limit_daily: Option<Option<u64>>,
        llm_config: Option<Option<&LLMConfig>>,
    ) -> Result<()> {
        let started = Instant::now();
        let payload = serde_json::json!({
            "system_prompt": system_prompt,
            "identity": identity,
            "soul": soul,
            "token_limit_total": token_limit_total,
            "token_limit_daily": token_limit_daily,
            "llm_config": llm_config.is_some(),
        });
        log_runtime_info("upsert_config", "started", agent_id, 0, payload.clone());
        if let Some(Some(cfg)) = llm_config {
            validate_llm_config(cfg)?;
        }
        let result = self
            .agent_store(agent_id)?
            .config_upsert(
                agent_id,
                system_prompt,
                identity,
                soul,
                token_limit_total,
                token_limit_daily,
                llm_config,
            )
            .await;
        match &result {
            Ok(_) => {
                self.invalidate_agent_llm(agent_id);
                log_runtime_info(
                    "upsert_config",
                    "completed",
                    agent_id,
                    started.elapsed().as_millis() as u64,
                    payload,
                );
            }
            Err(error) => log_runtime_error(
                "upsert_config",
                agent_id,
                started.elapsed().as_millis() as u64,
                error,
                payload,
            ),
        }
        result
    }
}
