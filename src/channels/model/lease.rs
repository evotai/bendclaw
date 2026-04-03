use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::Mutex;

use crate::channels::model::account::ChannelAccount;
use crate::channels::runtime::channel_registry::ChannelRegistry;
use crate::channels::runtime::channel_trait::InboundKind;
use crate::channels::runtime::diagnostics;
use crate::channels::runtime::supervisor::ChannelSupervisor;
use crate::kernel::lease::types::LeaseResource;
use crate::kernel::lease::types::ResourceEntry;
use crate::storage::dal::channel_account::repo::ChannelAccountRepo;
use crate::storage::pool::Pool;
use crate::storage::AgentDatabases;
use crate::types::Result;

pub struct ChannelLeaseResource {
    databases: Arc<AgentDatabases>,
    channels: Arc<ChannelRegistry>,
    supervisor: Arc<ChannelSupervisor>,
    discovered_configs: Mutex<HashMap<String, serde_json::Value>>,
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
            discovered_configs: Mutex::new(HashMap::new()),
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
        let mut configs = HashMap::new();

        for agent_id in &agent_ids {
            let pool = match self.databases.agent_pool(agent_id) {
                Ok(p) => p,
                Err(e) => {
                    diagnostics::log_channel_discover_skipped(agent_id, &e);
                    continue;
                }
            };

            let repo = ChannelAccountRepo::new(pool.clone());
            let accounts = match repo.list_by_agent(agent_id).await {
                Ok(a) => a,
                Err(e) => {
                    diagnostics::log_channel_list_failed(agent_id, &e);
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

                configs.insert(account.id.clone(), account.config.clone());
                entries.push(ResourceEntry {
                    id: account.id.clone(),
                    pool: pool.clone(),
                    lease_token: account.lease_token,
                    lease_node_id: account.lease_node_id,
                    lease_expires_at: account.lease_expires_at,
                    context: account.channel_type,
                    release_fn: None,
                });
            }
        }

        *self.discovered_configs.lock() = configs;
        Ok(entries)
    }

    async fn on_acquired(&self, entry: &ResourceEntry) -> Result<()> {
        let repo = ChannelAccountRepo::new(entry.pool.clone());
        let account = repo.load(&entry.id).await?.ok_or_else(|| {
            crate::types::ErrorCode::internal(format!(
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
        if !self.supervisor.is_alive(resource_id).await {
            return false;
        }
        let status = match self.supervisor.status().get(resource_id) {
            Some(s) => s,
            None => return false,
        };
        if !status.connected || status.is_stale() {
            return false;
        }
        let db_config = self.discovered_configs.lock().get(resource_id).cloned();
        match db_config {
            Some(db) => status.config == db,
            None => true,
        }
    }
}
