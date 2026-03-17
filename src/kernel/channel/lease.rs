use std::sync::Arc;

use async_trait::async_trait;

use crate::base::Result;
use crate::kernel::channel::account::ChannelAccount;
use crate::kernel::channel::plugin::InboundKind;
use crate::kernel::channel::registry::ChannelRegistry;
use crate::kernel::channel::supervisor::ChannelSupervisor;
use crate::kernel::lease::types::LeaseResource;
use crate::kernel::lease::types::ResourceEntry;
use crate::storage::dal::channel_account::repo::ChannelAccountRepo;
use crate::storage::pool::Pool;
use crate::storage::AgentDatabases;

pub struct ChannelLeaseResource {
    databases: Arc<AgentDatabases>,
    channels: Arc<ChannelRegistry>,
    supervisor: Arc<ChannelSupervisor>,
}

impl ChannelLeaseResource {
    pub fn new(
        databases: Arc<AgentDatabases>,
        channels: Arc<ChannelRegistry>,
        supervisor: Arc<ChannelSupervisor>,
    ) -> Self {
        Self {
            databases,
            channels,
            supervisor,
        }
    }
}

#[async_trait]
impl LeaseResource for ChannelLeaseResource {
    fn table(&self) -> &str {
        "channel_accounts"
    }

    fn lease_secs(&self) -> u64 {
        120
    }

    fn scan_interval_secs(&self) -> u64 {
        60
    }
    fn claim_condition(&self) -> Option<&str> {
        Some("enabled = true")
    }

    async fn discover(&self) -> Result<Vec<ResourceEntry>> {
        let agent_ids = self.databases.list_agent_ids().await?;
        let mut entries = Vec::new();

        for agent_id in &agent_ids {
            let pool = match self.databases.agent_pool(agent_id) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(agent_id, error = %e, "skip agent for channel lease discover");
                    continue;
                }
            };

            let repo = ChannelAccountRepo::new(pool.clone());
            let accounts = match repo.list_by_agent(agent_id).await {
                Ok(a) => a,
                Err(e) => {
                    tracing::warn!(agent_id, error = %e, "failed to list channel accounts");
                    continue;
                }
            };

            for account in accounts {
                if !account.enabled {
                    continue;
                }
                let has_receiver = self
                    .channels
                    .get(&account.channel_type)
                    .map(|e| matches!(e.inbound, InboundKind::Receiver(_)))
                    .unwrap_or(false);
                if !has_receiver {
                    continue;
                }

                entries.push(ResourceEntry {
                    id: account.id.clone(),
                    pool: pool.clone(),
                    lease_token: account.lease_token,
                    lease_node_id: account.lease_node_id,
                    lease_expires_at: account.lease_expires_at,
                    context: String::new(),
                    release_fn: None,
                });
            }
        }

        Ok(entries)
    }

    async fn on_acquired(&self, entry: &ResourceEntry) -> Result<()> {
        let repo = ChannelAccountRepo::new(entry.pool.clone());
        let account = repo.load(&entry.id).await?.ok_or_else(|| {
            crate::base::ErrorCode::internal(format!(
                "channel account '{}' disappeared after claim",
                entry.id
            ))
        })?;

        let channel_account = ChannelAccount {
            channel_account_id: account.id,
            channel_type: account.channel_type,
            external_account_id: account.account_id,
            agent_id: account.agent_id,
            user_id: account.user_id,
            config: account.config,
            enabled: account.enabled,
            created_at: account.created_at,
            updated_at: account.updated_at,
        };

        self.supervisor.start(&channel_account).await
    }

    async fn on_released(&self, resource_id: &str, _pool: &Pool) {
        self.supervisor.stop(resource_id).await;
    }

    async fn is_healthy(&self, resource_id: &str) -> bool {
        self.supervisor.is_alive(resource_id).await
    }
}
