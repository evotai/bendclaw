use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use crate::base::Result;
use crate::kernel::agent_store::memory_store::MemoryEntry;
use crate::kernel::agent_store::AgentStore;
use crate::kernel::runtime::Runtime;
use crate::kernel::skills::repository::DatabendSkillRepository;
use crate::kernel::skills::repository::SkillRepository;
use crate::kernel::skills::skill::Skill;
use crate::observability::redaction;
use crate::storage::dal::learning::record::LearningRecord;

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
    tracing::info!(
        log_kind = "server_log",
        stage = "runtime",
        command,
        status,
        agent_id,
        elapsed_ms,
        payload = %runtime_payload(payload),
        "runtime command"
    );
}

fn log_runtime_error(
    command: &str,
    agent_id: &str,
    elapsed_ms: u64,
    error: &impl std::fmt::Display,
    payload: serde_json::Value,
) {
    tracing::error!(
        log_kind = "server_log",
        stage = "runtime",
        command,
        status = "failed",
        agent_id,
        elapsed_ms,
        error = %error,
        payload = %runtime_payload(payload),
        "runtime command"
    );
}

impl Runtime {
    fn agent_store(&self, agent_id: &str) -> Result<Arc<AgentStore>> {
        self.require_ready()?;
        let pool = self.databases.agent_pool(agent_id)?;
        Ok(Arc::new(AgentStore::new(pool, self.llm.read().clone())))
    }

    pub async fn create_learning(&self, agent_id: &str, record: LearningRecord) -> Result<()> {
        let started = Instant::now();
        let payload = serde_json::json!({
            "learning_id": record.id.clone(),
            "user_id": record.user_id.clone(),
            "session_id": record.session_id.clone(),
            "title": record.title.clone(),
            "tags": record.tags.clone(),
            "source": record.source.clone(),
            "content": record.content.clone(),
        });
        log_runtime_info("create_learning", "started", agent_id, 0, payload.clone());
        let result = self.agent_store(agent_id)?.learning_insert(&record).await;
        match &result {
            Ok(_) => log_runtime_info(
                "create_learning",
                "completed",
                agent_id,
                started.elapsed().as_millis() as u64,
                payload,
            ),
            Err(error) => log_runtime_error(
                "create_learning",
                agent_id,
                started.elapsed().as_millis() as u64,
                error,
                payload,
            ),
        }
        result
    }

    pub async fn delete_learning(&self, agent_id: &str, learning_id: &str) -> Result<()> {
        let started = Instant::now();
        let payload = serde_json::json!({"learning_id": learning_id});
        log_runtime_info("delete_learning", "started", agent_id, 0, payload.clone());
        let result = self
            .agent_store(agent_id)?
            .learning_delete(learning_id)
            .await;
        match &result {
            Ok(_) => log_runtime_info(
                "delete_learning",
                "completed",
                agent_id,
                started.elapsed().as_millis() as u64,
                payload,
            ),
            Err(error) => log_runtime_error(
                "delete_learning",
                agent_id,
                started.elapsed().as_millis() as u64,
                error,
                payload,
            ),
        }
        result
    }

    pub async fn delete_session(&self, agent_id: &str, session_id: &str) -> Result<()> {
        let started = Instant::now();
        let had_live_session = self.sessions.get(session_id).is_some();
        let payload = serde_json::json!({
            "session_id": session_id,
            "had_live_session": had_live_session,
        });
        log_runtime_info("delete_session", "started", agent_id, 0, payload.clone());
        if let Some(session) = self.sessions.get(session_id) {
            session.close().await;
            self.sessions.remove(session_id);
        }
        let result = self.agent_store(agent_id)?.session_delete(session_id).await;
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
        let repo = crate::storage::RunRepo::new(pool);

        let result = async {
            if let Some(record) = repo.load(run_id).await? {
                if let Some(session) = self.sessions.get(&record.session_id) {
                    let _ = session.cancel_run(run_id);
                }
                if record.status == crate::storage::RunStatus::Pending.as_str() {
                    repo.update_status(run_id, crate::storage::RunStatus::Cancelled)
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
        display_name: Option<&str>,
        description: Option<&str>,
        identity: Option<&str>,
        soul: Option<&str>,
        token_limit_total: Option<Option<u64>>,
        token_limit_daily: Option<Option<u64>>,
        env: Option<&HashMap<String, String>>,
        notes: Option<&str>,
        label: Option<&str>,
    ) -> Result<u32> {
        let started = Instant::now();
        let payload = serde_json::json!({
            "display_name": display_name,
            "description": description,
            "system_prompt": system_prompt,
            "identity": identity,
            "soul": soul,
            "token_limit_total": token_limit_total,
            "token_limit_daily": token_limit_daily,
            "env": env,
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
        let result = self
            .agent_store(agent_id)?
            .config_update_with_version(
                agent_id,
                system_prompt,
                display_name,
                description,
                identity,
                soul,
                token_limit_total,
                token_limit_daily,
                env,
                notes,
                label,
            )
            .await;
        match &result {
            Ok(version) => log_runtime_info(
                "update_config_with_version",
                "completed",
                agent_id,
                started.elapsed().as_millis() as u64,
                serde_json::json!({"payload": payload, "version": version}),
            ),
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

    pub async fn create_memory(
        &self,
        agent_id: &str,
        user_id: &str,
        entry: MemoryEntry,
    ) -> Result<()> {
        let started = Instant::now();
        let payload = serde_json::json!({
            "memory_id": entry.id.clone(),
            "user_id": user_id,
            "scope": format!("{:?}", entry.scope),
            "session_id": entry.session_id.clone(),
            "key": entry.key.clone(),
            "content": entry.content.clone(),
        });
        log_runtime_info("create_memory", "started", agent_id, 0, payload.clone());
        let result = self
            .agent_store(agent_id)?
            .memory_write(user_id, entry)
            .await;
        match &result {
            Ok(_) => log_runtime_info(
                "create_memory",
                "completed",
                agent_id,
                started.elapsed().as_millis() as u64,
                payload,
            ),
            Err(error) => log_runtime_error(
                "create_memory",
                agent_id,
                started.elapsed().as_millis() as u64,
                error,
                payload,
            ),
        }
        result
    }

    pub async fn delete_memory(
        &self,
        agent_id: &str,
        user_id: &str,
        memory_id: &str,
    ) -> Result<()> {
        let started = Instant::now();
        let payload = serde_json::json!({
            "memory_id": memory_id,
            "user_id": user_id,
        });
        log_runtime_info("delete_memory", "started", agent_id, 0, payload.clone());
        let result = self
            .agent_store(agent_id)?
            .memory_delete(user_id, memory_id)
            .await;
        match &result {
            Ok(_) => log_runtime_info(
                "delete_memory",
                "completed",
                agent_id,
                started.elapsed().as_millis() as u64,
                payload,
            ),
            Err(error) => log_runtime_error(
                "delete_memory",
                agent_id,
                started.elapsed().as_millis() as u64,
                error,
                payload,
            ),
        }
        result
    }

    pub async fn create_skill(&self, agent_id: &str, skill: Skill) -> Result<()> {
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
        log_runtime_info("create_skill", "started", agent_id, 0, payload.clone());
        self.require_ready()?;
        let pool = self.databases.agent_pool(agent_id)?;
        let store = DatabendSkillRepository::new(pool);
        let result = async {
            store.save(&skill).await?;
            self.skills.insert(&skill);
            Ok(())
        }
        .await;
        match &result {
            Ok(_) => log_runtime_info(
                "create_skill",
                "completed",
                agent_id,
                started.elapsed().as_millis() as u64,
                payload,
            ),
            Err(error) => log_runtime_error(
                "create_skill",
                agent_id,
                started.elapsed().as_millis() as u64,
                error,
                payload,
            ),
        }
        result
    }

    pub async fn delete_skill(
        &self,
        agent_id: &str,
        user_id: &str,
        skill_name: &str,
    ) -> Result<()> {
        let started = Instant::now();
        let payload = serde_json::json!({
            "skill_name": skill_name,
            "user_id": user_id,
        });
        log_runtime_info("delete_skill", "started", agent_id, 0, payload.clone());
        self.require_ready()?;
        let pool = self.databases.agent_pool(agent_id)?;
        let store = DatabendSkillRepository::new(pool);
        let result = async {
            store
                .remove(skill_name, Some(agent_id), Some(user_id))
                .await?;
            self.skills.evict(skill_name);
            Ok(())
        }
        .await;
        match &result {
            Ok(_) => log_runtime_info(
                "delete_skill",
                "completed",
                agent_id,
                started.elapsed().as_millis() as u64,
                payload,
            ),
            Err(error) => log_runtime_error(
                "delete_skill",
                agent_id,
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
    ) -> Result<()> {
        let started = Instant::now();
        let payload = serde_json::json!({
            "session_id": session_id,
            "user_id": user_id,
            "title": title,
            "session_state": session_state,
            "meta": meta,
        });
        log_runtime_info("upsert_session", "started", agent_id, 0, payload.clone());
        let store = self.agent_store(agent_id)?;
        let result = async {
            store
                .session_upsert(session_id, agent_id, user_id, title, meta)
                .await?;
            if let Some(state) = session_state {
                store.session_update_state(session_id, state).await?;
            }
            Ok(())
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

    pub async fn upsert_config(
        &self,
        agent_id: &str,
        system_prompt: Option<&str>,
        display_name: Option<&str>,
        description: Option<&str>,
        identity: Option<&str>,
        soul: Option<&str>,
        token_limit_total: Option<Option<u64>>,
        token_limit_daily: Option<Option<u64>>,
        env: Option<&HashMap<String, String>>,
    ) -> Result<()> {
        let started = Instant::now();
        let payload = serde_json::json!({
            "system_prompt": system_prompt,
            "display_name": display_name,
            "description": description,
            "identity": identity,
            "soul": soul,
            "token_limit_total": token_limit_total,
            "token_limit_daily": token_limit_daily,
            "env": env,
        });
        log_runtime_info("upsert_config", "started", agent_id, 0, payload.clone());
        let result = self
            .agent_store(agent_id)?
            .config_upsert(
                agent_id,
                system_prompt,
                display_name,
                description,
                identity,
                soul,
                token_limit_total,
                token_limit_daily,
                env,
            )
            .await;
        match &result {
            Ok(_) => log_runtime_info(
                "upsert_config",
                "completed",
                agent_id,
                started.elapsed().as_millis() as u64,
                payload,
            ),
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
