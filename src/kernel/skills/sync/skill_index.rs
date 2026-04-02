//! SkillIndex — single read-side entry point for skill visibility + filesystem mirror.
//!
//! Owns:
//! - `reconcile()`: writes/evicts filesystem mirror from DB state
//! - `build_visible_index()`: assembles visible skills from projected filesystem
//! - All public read methods delegate to `build_visible_index()`

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use crate::base::Result;
use crate::config::HubConfig;
use crate::kernel::skills::definition::skill::Skill;
use crate::kernel::skills::definition::skill::SkillScope;
use crate::kernel::skills::definition::skill::SkillSource;
use crate::kernel::skills::definition::tool_key;
use crate::kernel::skills::diagnostics;
use crate::kernel::skills::fs::load_skill_tree;
use crate::kernel::skills::fs::LoadedSkill;
use crate::kernel::skills::sources::hub;
use crate::kernel::skills::sources::remote;
use crate::kernel::skills::store::SharedSkillStore;
use crate::kernel::subscriptions::SubscriptionStore;

pub struct SkillIndex {
    workspace_root: PathBuf,
    store: Arc<dyn SharedSkillStore>,
    sub_store: Arc<dyn SubscriptionStore>,
    hub_config: Option<HubConfig>,
}

impl SkillIndex {
    pub fn new(
        workspace_root: PathBuf,
        store: Arc<dyn SharedSkillStore>,
        sub_store: Arc<dyn SubscriptionStore>,
        hub_config: Option<HubConfig>,
    ) -> Self {
        let _ = std::fs::create_dir_all(hub::paths::hub_dir(&workspace_root));
        Self {
            workspace_root,
            store,
            sub_store,
            hub_config,
        }
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    pub fn hub_config(&self) -> Option<&HubConfig> {
        self.hub_config.as_ref()
    }

    // ── Projection: reconcile DB → filesystem mirror ──

    pub fn ensure_hub(&self) {
        let Some(hub_cfg) = &self.hub_config else {
            return;
        };
        hub::sync::ensure(
            &self.workspace_root,
            &hub_cfg.repo_url,
            hub_cfg.sync_interval_secs,
        );
    }

    /// Reconcile filesystem mirror for a user against DB state.
    /// Called after every command AND by the periodic sync loop.
    pub async fn reconcile(&self, user_id: &str) -> Result<()> {
        self.ensure_hub();

        let owned = match self.store.list(user_id).await {
            Ok(skills) => skills,
            Err(e) => {
                diagnostics::log_skill_sync_list_failed(user_id, &e);
                return Err(e);
            }
        };
        let (subscribed, subscribed_ok) = match self.sub_store.list(user_id, "skill").await {
            Ok(subs) => {
                let mut skills = Vec::new();
                for sub in &subs {
                    if let Ok(Some(skill)) = self.store.get(&sub.owner_id, &sub.resource_key).await
                    {
                        skills.push(skill);
                    }
                }
                (skills, true)
            }
            Err(e) => {
                diagnostics::log_skill_sync_list_failed(user_id, &e);
                (Vec::new(), false)
            }
        };

        // (owner_key, skill_name) — empty owner_key for owned
        let mut live_keys: HashSet<(String, String)> = HashSet::new();
        // Track whether we have a complete picture for eviction
        let owned_ok = true; // if we got here, owned list succeeded

        // Mirror owned skills — fetch full skill (with files) for each
        for skill_summary in &owned {
            live_keys.insert((String::new(), skill_summary.name.clone()));
            let db_checksum = skill_summary.compute_sha256();
            if let Some(disk) = remote::writer::read_disk_checksum(
                &self.workspace_root,
                user_id,
                &skill_summary.name,
            ) {
                if disk == db_checksum {
                    continue;
                }
            }
            // Fetch full skill with files for writing
            let skill = match self.store.get(user_id, &skill_summary.name).await {
                Ok(Some(s)) => s,
                _ => continue,
            };
            let remote_dir = remote::paths::remote_dir(&self.workspace_root, user_id);
            let _ = std::fs::create_dir_all(&remote_dir);
            remote::writer::write_skill(&self.workspace_root, user_id, &skill);
        }

        // Mirror subscribed skills — fetch full skill (with files) for each
        for skill_summary in &subscribed {
            let owner_id = &skill_summary.user_id;
            live_keys.insert((owner_id.clone(), skill_summary.name.clone()));
            let db_checksum = skill_summary.compute_sha256();
            if let Some(disk) = remote::writer::read_subscribed_disk_checksum(
                &self.workspace_root,
                user_id,
                owner_id,
                &skill_summary.name,
            ) {
                if disk == db_checksum {
                    continue;
                }
            }
            let skill = match self.store.get(owner_id, &skill_summary.name).await {
                Ok(Some(s)) => s,
                _ => continue,
            };
            let sub_dir = remote::paths::subscribed_dir(&self.workspace_root, user_id, owner_id);
            let _ = std::fs::create_dir_all(&sub_dir);
            remote::writer::write_subscribed_skill(&self.workspace_root, user_id, owner_id, &skill);
        }

        self.evict_stale(user_id, &live_keys, owned_ok, subscribed_ok);
        Ok(())
    }

    // ── Public read API — all delegate to build_visible_index ──

    pub fn visible_skills(&self, user_id: &str) -> Vec<Skill> {
        let mut entries: Vec<(String, Skill)> = self
            .build_visible_index(user_id)
            .into_iter()
            .map(|(key, l)| (key, l.skill))
            .collect();
        entries.sort_by(|(ka, _), (kb, _)| ka.cmp(kb));
        entries.into_iter().map(|(_, s)| s).collect()
    }

    pub fn resolve(&self, user_id: &str, tool_key_str: &str) -> Option<Skill> {
        self.build_visible_index(user_id)
            .remove(tool_key_str)
            .map(|l| l.skill)
    }

    pub fn read_skill(&self, user_id: &str, path: &str) -> Option<String> {
        if path.contains("..") {
            return None;
        }
        let index = self.build_visible_index(user_id);
        // Try exact match first
        if let Some(loaded) = index.get(path) {
            return Some(loaded.skill.content.clone());
        }
        // Longest prefix match: progressively shorten from right at each /
        for (idx, _) in path.match_indices('/').rev() {
            let key = &path[..idx];
            if let Some(loaded) = index.get(key) {
                return loaded.read_doc(&path[idx + 1..]);
            }
        }
        None
    }

    pub fn host_script_path(&self, user_id: &str, tool_key_str: &str) -> Option<PathBuf> {
        self.build_visible_index(user_id)
            .remove(tool_key_str)
            .and_then(|l| l.script_path())
    }

    pub fn script_path(&self, user_id: &str, tool_key_str: &str) -> Option<String> {
        let loaded = self.build_visible_index(user_id).remove(tool_key_str)?;
        let host_script = loaded.script_path()?;
        let hub_dir = hub::paths::hub_dir(&self.workspace_root);
        if let Ok(rel) = host_script.strip_prefix(&hub_dir) {
            return Some(format!("/workspace/skills/.hub/{}", rel.to_string_lossy()));
        }
        let users_dir = self.workspace_root.join("users");
        if let Ok(rel) = host_script.strip_prefix(&users_dir) {
            return Some(format!("/workspace/users/{}", rel.to_string_lossy()));
        }
        None
    }

    pub fn loaded_skills(&self) -> Vec<LoadedSkill> {
        let mut all = self.load_hub_skills();
        let users_dir = self.workspace_root.join("users");
        if let Ok(entries) = std::fs::read_dir(&users_dir) {
            for entry in entries.flatten() {
                let user_dir = entry.path();
                if !user_dir.is_dir() {
                    continue;
                }
                let remote_dir = user_dir.join("skills").join(".remote");
                all.extend(Self::load_from_dir(&remote_dir));
                let subscribed_base = remote_dir.join("subscribed");
                if let Ok(owners) = std::fs::read_dir(&subscribed_base) {
                    for owner_entry in owners.flatten() {
                        let owner_path = owner_entry.path();
                        if !owner_path.is_dir() {
                            continue;
                        }
                        all.extend(Self::load_from_dir(&owner_path));
                    }
                }
            }
        }
        all
    }

    pub fn get_hub(&self, name: &str) -> Option<Skill> {
        let hub_skill_dir = hub::paths::hub_dir(&self.workspace_root).join(name);
        let mut loaded = load_skill_tree(&hub_skill_dir, name)?;
        loaded.skill.source = SkillSource::Hub;
        loaded.skill.scope = SkillScope::Shared;
        Some(loaded.skill)
    }

    /// Return only hub-sourced skills.
    pub fn hub_skills(&self) -> Vec<Skill> {
        self.load_hub_skills()
            .into_iter()
            .map(|mut l| {
                l.skill.source = SkillSource::Hub;
                l.skill.scope = SkillScope::Shared;
                l.skill
            })
            .collect()
    }

    /// Last successful hub sync time.
    pub fn hub_last_sync(&self) -> Option<std::time::SystemTime> {
        hub::sync::last_sync_time(&self.workspace_root)
    }

    // ── Private: single visibility logic ──

    /// Build visible skill index. Single source of precedence.
    /// Key = tool key (bare for owned/hub, "owner/name" for subscribed).
    fn build_visible_index(&self, user_id: &str) -> HashMap<String, LoadedSkill> {
        let mut index: HashMap<String, LoadedSkill> = HashMap::new();

        // 1. Hub skills keyed by bare name (lowest priority)
        for mut loaded in self.load_hub_skills() {
            loaded.skill.source = SkillSource::Hub;
            loaded.skill.scope = SkillScope::Shared;
            index.insert(loaded.skill.name.clone(), loaded);
        }

        // 2. Owned skills override hub by bare name
        let remote_dir = remote::paths::remote_dir(&self.workspace_root, user_id);
        for loaded in Self::load_from_dir(&remote_dir) {
            index.insert(loaded.skill.name.clone(), loaded);
        }

        // 3. Subscribed skills keyed by "owner/name"
        let subscribed_base = remote_dir.join("subscribed");
        if let Ok(owners) = std::fs::read_dir(&subscribed_base) {
            for owner_entry in owners.flatten() {
                let owner_path = owner_entry.path();
                if !owner_path.is_dir() {
                    continue;
                }
                let owner_id = match owner_path.file_name().and_then(|n| n.to_str()) {
                    Some(n) => n,
                    None => continue,
                };
                for loaded in Self::load_from_dir(&owner_path) {
                    let key = tool_key::format_subscribed(owner_id, &loaded.skill.name);
                    index.insert(key, loaded);
                }
            }
        }

        index
    }

    // ── Private helpers ──

    fn load_hub_skills(&self) -> Vec<LoadedSkill> {
        let hub_dir = hub::paths::hub_dir(&self.workspace_root);
        crate::kernel::skills::fs::load_skills(&hub_dir)
    }

    fn load_from_dir(dir: &Path) -> Vec<LoadedSkill> {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return Vec::new(),
        };
        let mut skills = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let dir_name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) if !n.starts_with('_') && !n.starts_with('.') => n.to_string(),
                _ => continue,
            };
            if let Some(loaded) = load_skill_tree(&path, &dir_name) {
                skills.push(loaded);
            }
        }
        skills
    }

    fn evict_stale(
        &self,
        user_id: &str,
        live_keys: &HashSet<(String, String)>,
        evict_owned: bool,
        evict_subscribed: bool,
    ) {
        // Evict owned skills — only if we successfully listed owned
        if evict_owned {
            let remote_dir = remote::paths::remote_dir(&self.workspace_root, user_id);
            if let Ok(entries) = std::fs::read_dir(&remote_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }
                    let name = match path.file_name().and_then(|n| n.to_str()) {
                        Some(n) if !n.starts_with('.') && n != "subscribed" => n.to_string(),
                        _ => continue,
                    };
                    if !live_keys.contains(&(String::new(), name)) {
                        let _ = std::fs::remove_dir_all(&path);
                    }
                }
            }
        }
        // Evict subscribed skills — only if we successfully listed subscribed
        if evict_subscribed {
            let remote_dir = remote::paths::remote_dir(&self.workspace_root, user_id);
            let subscribed_base = remote_dir.join("subscribed");
            if let Ok(owner_entries) = std::fs::read_dir(&subscribed_base) {
                for owner_entry in owner_entries.flatten() {
                    let owner_dir = owner_entry.path();
                    if !owner_dir.is_dir() {
                        continue;
                    }
                    let owner_id = match owner_dir.file_name().and_then(|n| n.to_str()) {
                        Some(n) => n.to_string(),
                        None => continue,
                    };
                    if let Ok(skill_entries) = std::fs::read_dir(&owner_dir) {
                        for skill_entry in skill_entries.flatten() {
                            let skill_dir = skill_entry.path();
                            if !skill_dir.is_dir() {
                                continue;
                            }
                            let skill_name = match skill_dir.file_name().and_then(|n| n.to_str()) {
                                Some(n) if !n.starts_with('.') => n.to_string(),
                                _ => continue,
                            };
                            if !live_keys.contains(&(owner_id.clone(), skill_name)) {
                                let _ = std::fs::remove_dir_all(&skill_dir);
                            }
                        }
                    }
                }
            }
        }
    }
}
