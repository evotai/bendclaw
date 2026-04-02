//! OrgServices — aggregates org-level (evotai_meta) shared services.

use std::sync::Arc;

use crate::kernel::memory::store::SharedMemoryStore;
use crate::kernel::memory::MemoryService;
use crate::kernel::runtime::agent_config::AgentConfig;
use crate::kernel::skills::store::DatabendSharedSkillStore;
use crate::kernel::skills::sync::SkillCatalog;
use crate::kernel::skills::sync::SkillManager;
use crate::kernel::subscriptions::SharedSubscriptionStore;
use crate::kernel::subscriptions::SubscriptionStore;
use crate::kernel::variables::service::VariableService;
use crate::kernel::variables::store::SharedVariableStore;
use crate::llm::provider::LLMProvider;
use crate::storage::pool::Pool;

pub struct OrgServices {
    variables: Arc<VariableService>,
    catalog: Arc<SkillCatalog>,
    manager: Arc<SkillManager>,
    memory: Option<Arc<MemoryService>>,
    subscriptions: Arc<dyn SubscriptionStore>,
}

impl OrgServices {
    pub fn new(
        meta_pool: Pool,
        catalog: Arc<SkillCatalog>,
        config: &AgentConfig,
        llm: Arc<dyn LLMProvider>,
    ) -> Self {
        let sub_store: Arc<dyn SubscriptionStore> =
            Arc::new(SharedSubscriptionStore::new(meta_pool.clone()));

        let variable_store = Arc::new(SharedVariableStore::new(meta_pool.clone()));
        let variables = Arc::new(VariableService::new(variable_store, sub_store.clone()));

        let skill_store = Arc::new(DatabendSharedSkillStore::new(meta_pool.clone()));
        let manager = Arc::new(SkillManager::new(
            skill_store,
            sub_store.clone(),
            catalog.clone(),
        ));

        let memory = if config.memory.enabled {
            let store = Arc::new(SharedMemoryStore::new(meta_pool));
            let model: Arc<str> = llm.default_model().into();
            Some(Arc::new(MemoryService::new(store, llm, model)))
        } else {
            None
        };

        Self {
            variables,
            catalog,
            manager,
            memory,
            subscriptions: sub_store,
        }
    }

    pub fn variables(&self) -> &Arc<VariableService> {
        &self.variables
    }

    pub(crate) fn catalog(&self) -> &Arc<SkillCatalog> {
        &self.catalog
    }

    pub fn manager(&self) -> &Arc<SkillManager> {
        &self.manager
    }

    pub fn memory(&self) -> Option<&Arc<MemoryService>> {
        self.memory.as_ref()
    }

    pub fn subscriptions(&self) -> &Arc<dyn SubscriptionStore> {
        &self.subscriptions
    }
}

impl super::session_org::SessionOrgServices for OrgServices {
    fn list_skills(&self, user_id: &str) -> Vec<crate::kernel::skills::definition::skill::Skill> {
        self.catalog.visible_skills(user_id)
    }
    fn memory(&self) -> Option<Arc<MemoryService>> {
        self.memory.clone()
    }
}
