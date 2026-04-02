use crate::kernel::runtime::Runtime;
use crate::types::Result;
impl Runtime {
    pub fn agent_config_store(
        &self,
        agent_id: &str,
    ) -> Result<crate::storage::dal::agent_config::repo::AgentConfigStore> {
        let pool = self.databases.agent_pool(agent_id)?;
        Ok(crate::storage::dal::agent_config::repo::AgentConfigStore::new(pool))
    }

    pub async fn setup_agent(&self, agent_id: &str) -> Result<()> {
        self.require_ready()?;
        let db_name = self.databases.agent_database_name(agent_id)?;
        let pool = self.database();
        pool.exec(&format!("CREATE DATABASE IF NOT EXISTS `{db_name}`"))
            .await?;
        let agent_pool = self.databases.agent_pool(agent_id)?;
        crate::storage::migrator::run_agent(&agent_pool).await;

        let config_store =
            crate::storage::dal::agent_config::repo::AgentConfigStore::new(agent_pool);
        if config_store.get(agent_id).await?.is_none() {
            config_store
                .upsert(agent_id, None, None, None, None, None, None)
                .await?;
        }

        Ok(())
    }

    pub fn agent_database_name(&self, agent_id: &str) -> Result<String> {
        self.databases.agent_database_name(agent_id)
    }
}
