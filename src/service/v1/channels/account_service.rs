use super::http::ChannelAccountView;
use super::http::CreateChannelAccountRequest;
use crate::base::new_id;
use crate::kernel::channel::account::ChannelAccount;
use crate::service::error::Result;
use crate::service::error::ServiceError;
use crate::service::state::AppState;
use crate::storage::dal::channel_account::record::ChannelAccountRecord;
use crate::storage::dal::channel_account::repo::ChannelAccountRepo;

pub struct ChannelAccountService {
    state: AppState,
}

impl ChannelAccountService {
    pub fn new(state: &AppState) -> Self {
        Self {
            state: state.clone(),
        }
    }

    pub async fn create(
        &self,
        agent_id: &str,
        req: CreateChannelAccountRequest,
    ) -> Result<ChannelAccountView> {
        let registry = self.state.runtime.channels();
        let entry = registry.get(&req.channel_type).ok_or_else(|| {
            ServiceError::BadRequest(format!("unknown channel type: {}", req.channel_type))
        })?;
        entry
            .plugin
            .validate_config(&req.config)
            .map_err(|e| ServiceError::BadRequest(format!("invalid channel config: {e}")))?;

        let pool = self.state.runtime.databases().agent_pool(agent_id)?;
        let repo = ChannelAccountRepo::new(pool.clone());

        let id = new_id();
        let external_account_id = req.external_account_id.unwrap_or_else(new_id);

        let record = ChannelAccountRecord {
            id: id.clone(),
            channel_type: req.channel_type.clone(),
            account_id: external_account_id,
            agent_id: agent_id.to_string(),
            user_id: req.user_id.clone(),
            config: req.config,
            enabled: req.enabled.unwrap_or(true),
            lease_node_id: None,
            lease_token: None,
            lease_expires_at: None,
            created_at: String::new(),
            updated_at: String::new(),
        };

        repo.insert(&record).await?;

        let saved = repo
            .load(&id)
            .await?
            .ok_or_else(|| ServiceError::Internal("failed to load created account".to_string()))?;

        let account = record_to_domain(&saved);
        // Lease service will auto-discover and start the receiver.
        Ok(domain_to_view(account))
    }

    pub async fn list(&self, agent_id: &str) -> Result<Vec<ChannelAccountView>> {
        let pool = self.state.runtime.databases().agent_pool(agent_id)?;
        let repo = ChannelAccountRepo::new(pool);
        let records = repo.list_by_agent(agent_id).await?;
        Ok(records
            .into_iter()
            .map(|r| domain_to_view(record_to_domain(&r)))
            .collect())
    }

    pub async fn get(
        &self,
        agent_id: &str,
        channel_account_id: &str,
    ) -> Result<ChannelAccountView> {
        let pool = self.state.runtime.databases().agent_pool(agent_id)?;
        let repo = ChannelAccountRepo::new(pool);
        let record = repo.load(channel_account_id).await?.ok_or_else(|| {
            ServiceError::AgentNotFound(format!("channel account '{channel_account_id}' not found"))
        })?;
        Ok(domain_to_view(record_to_domain(&record)))
    }

    pub async fn delete(&self, agent_id: &str, channel_account_id: &str) -> Result<()> {
        let pool = self.state.runtime.databases().agent_pool(agent_id)?;
        let repo = ChannelAccountRepo::new(pool);

        if let Ok(Some(_)) = repo.load(channel_account_id).await {
            self.state
                .runtime
                .supervisor()
                .stop(channel_account_id)
                .await;
            let _ = repo.release_lease(channel_account_id).await;
        }

        repo.delete(channel_account_id).await?;
        Ok(())
    }
}

pub fn record_to_domain(r: &ChannelAccountRecord) -> ChannelAccount {
    ChannelAccount {
        channel_account_id: r.id.clone(),
        channel_type: r.channel_type.clone(),
        external_account_id: r.account_id.clone(),
        agent_id: r.agent_id.clone(),
        user_id: r.user_id.clone(),
        config: r.config.clone(),
        enabled: r.enabled,
        created_at: r.created_at.clone(),
        updated_at: r.updated_at.clone(),
    }
}

pub fn domain_to_view(a: ChannelAccount) -> ChannelAccountView {
    ChannelAccountView {
        id: a.channel_account_id,
        channel_type: a.channel_type,
        external_account_id: a.external_account_id,
        agent_id: a.agent_id,
        user_id: a.user_id,
        config: mask_sensitive_config(a.config),
        enabled: a.enabled,
        created_at: a.created_at,
        updated_at: a.updated_at,
    }
}

fn mask_sensitive_config(config: serde_json::Value) -> serde_json::Value {
    match config {
        serde_json::Value::Object(map) => {
            let masked = map
                .into_iter()
                .map(|(k, v)| {
                    let sensitive = ["token", "secret", "password", "key"]
                        .iter()
                        .any(|kw| k.to_lowercase().contains(kw));
                    let v = if sensitive && v.is_string() {
                        serde_json::Value::String("***".into())
                    } else {
                        v
                    };
                    (k, v)
                })
                .collect();
            serde_json::Value::Object(masked)
        }
        other => other,
    }
}
