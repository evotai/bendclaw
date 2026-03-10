//! Shared test infrastructure for `SkillCatalogImpl` integration tests.
//!
//! ```text
//!  Remote                          Local_a          Local_b
//!  .publish(skill) ──┐              .sync()          .sync()
//!  .remove(name)   ──┤  Databend  ──────────────────────────
//!                    └─────────────────────────────────────
//! ```
//!
//! `Remote` simulates an external writer (admin API, deploy pipeline).
//! `Local`  simulates one agent node with its own local filesystem mirror.
//! `Cluster` = one `Remote` + two independent `Local` nodes.

use std::sync::Arc;
use std::time::SystemTime;

use anyhow::Result;
use bendclaw::kernel::skills::catalog::SkillCatalog;
use bendclaw::kernel::skills::catalog::SkillCatalogImpl;
use bendclaw::kernel::skills::repository::DatabendSkillRepository;
use bendclaw::kernel::skills::repository::SkillRepository;
use bendclaw::kernel::skills::skill::Skill;
use bendclaw::kernel::skills::skill::SkillFile;
use bendclaw::kernel::skills::skill::SkillSource;
use bendclaw::storage::AgentDatabases;
use bendclaw_test_harness::setup::pool;
use bendclaw_test_harness::setup::uid;
use tempfile::TempDir;

// ── Skill builders ────────────────────────────────────────────────────────────

/// Skill with no files; description auto-derived from name.
pub fn skill_plain(name: &str, version: &str, content: &str) -> Skill {
    Skill {
        name: name.to_string(),
        version: version.to_string(),
        description: format!("description of {name}"),
        scope: Default::default(),
        source: SkillSource::Agent,
        agent_id: None,
        user_id: None,
        timeout: 45,
        executable: false,
        parameters: vec![],
        content: content.to_string(),
        files: vec![],
        requires: None,
    }
}

/// Skill with custom description and timeout; no files.
pub fn skill_with_meta(
    name: &str,
    version: &str,
    description: &str,
    timeout: u64,
    content: &str,
) -> Skill {
    Skill {
        name: name.to_string(),
        version: version.to_string(),
        description: description.to_string(),
        scope: Default::default(),
        source: SkillSource::Agent,
        agent_id: None,
        user_id: None,
        timeout,
        executable: false,
        parameters: vec![],
        content: content.to_string(),
        files: vec![],
        requires: None,
    }
}

/// Skill with an explicit file list; `executable` is derived from file extensions.
pub fn skill_with_files(name: &str, version: &str, files: Vec<(&str, &str)>) -> Skill {
    Skill {
        name: name.to_string(),
        version: version.to_string(),
        description: format!("description of {name}"),
        scope: Default::default(),
        source: SkillSource::Agent,
        agent_id: None,
        user_id: None,
        timeout: 30,
        executable: files
            .iter()
            .any(|(p, _)| p.ends_with(".py") || p.ends_with(".sh")),
        parameters: vec![],
        content: format!("# {name}"),
        files: files
            .into_iter()
            .map(|(path, body)| SkillFile {
                path: path.to_string(),
                body: body.to_string(),
            })
            .collect(),
        requires: None,
    }
}

// ── Remote ────────────────────────────────────────────────────────────────────

/// External writer — publishes and removes skills in Databend.
/// Holds no local cache or filesystem state.
pub struct Remote {
    store: DatabendSkillRepository,
}

impl Remote {
    pub async fn publish(&self, skill: &Skill) -> Result<()> {
        self.store.save(skill).await?;
        Ok(())
    }

    pub async fn remove(&self, name: &str) -> Result<()> {
        self.store.remove(name, None, None).await?;
        Ok(())
    }
}

// ── Local ─────────────────────────────────────────────────────────────────────

/// One agent node — wraps `SkillCatalogImpl` and its own local `TempDir`.
/// Learns about remote changes only via `sync()`.
pub struct Local {
    pub store: SkillCatalogImpl,
    pub dir: TempDir,
}

impl Local {
    // ── lifecycle ──

    pub async fn sync(&self) -> Result<()> {
        self.store.sync().await?;
        Ok(())
    }

    // ── in-memory cache reads ──

    pub fn skill_count(&self) -> usize {
        self.store.for_agent("", "").len()
    }

    pub fn is_executable(&self, name: &str) -> bool {
        self.store.get(name).map(|s| s.executable).unwrap_or(false)
    }

    // ── cache assertions ──

    pub fn assert_cached(&self, name: &str) {
        assert!(
            self.store.get(name).is_some(),
            "{name} must be in the in-memory cache"
        );
    }

    pub fn assert_not_cached(&self, name: &str) {
        assert!(
            self.store.get(name).is_none(),
            "{name} must NOT be in the in-memory cache"
        );
    }

    pub fn assert_version(&self, name: &str, expected: &str) {
        let got = self.store.get(name).map(|s| s.version).unwrap_or_default();
        assert_eq!(got, expected, "{name} version mismatch");
    }

    pub fn assert_content(&self, name: &str, expected: &str) {
        let got = self.store.get(name).map(|s| s.content).unwrap_or_default();
        assert_eq!(got, expected, "{name} content mismatch");
    }

    pub fn assert_description(&self, name: &str, expected: &str) {
        let got = self
            .store
            .get(name)
            .map(|s| s.description)
            .unwrap_or_default();
        assert_eq!(got, expected, "{name} description mismatch");
    }

    pub fn assert_timeout(&self, name: &str, expected: u64) {
        let got = self.store.get(name).map(|s| s.timeout).unwrap_or(0);
        assert_eq!(got, expected, "{name} timeout mismatch");
    }

    /// Direct cache read, available so test files don't need to import SkillRegistry.
    pub fn get(&self, name: &str) -> Option<Skill> {
        self.store.get(name)
    }

    // ── filesystem assertions ──

    /// Remote skills are mirrored under `.remote/` subdirectory.
    pub fn skill_dir(&self, name: &str) -> std::path::PathBuf {
        self.dir.path().join(".remote").join(name)
    }

    pub fn assert_skill_dir_exists(&self, name: &str) {
        assert!(
            self.skill_dir(name).is_dir(),
            "skill directory {name}/ must exist on disk"
        );
    }

    pub fn assert_skill_dir_absent(&self, name: &str) {
        assert!(
            !self.skill_dir(name).exists(),
            "skill directory {name}/ must NOT exist on disk"
        );
    }

    /// Assert a file inside the skill's local dir has exactly `expected` content.
    pub fn assert_file(&self, skill: &str, rel_path: &str, expected: &str) {
        let path = self.skill_dir(skill).join(rel_path);
        assert!(
            path.exists(),
            "expected file '{rel_path}' to exist in {skill}/ but it was absent"
        );
        let actual = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read '{rel_path}': {e:#}"));
        assert_eq!(
            actual, expected,
            "'{rel_path}' in {skill}/ content mismatch"
        );
    }

    /// Assert a path inside the skill's local dir does NOT exist
    /// (applies to both files and directories).
    pub fn assert_no_path(&self, skill: &str, rel_path: &str) {
        assert!(
            !self.skill_dir(skill).join(rel_path).exists(),
            "expected '{rel_path}' to be ABSENT in {skill}/ but it was present"
        );
    }

    /// Return the mtime of a file for rewrite-detection comparisons.
    pub fn mtime(&self, skill: &str, rel_path: &str) -> SystemTime {
        std::fs::metadata(self.skill_dir(skill).join(rel_path))
            .unwrap_or_else(|e| panic!("cannot stat '{rel_path}' in {skill}/: {e:#}"))
            .modified()
            .expect("mtime not supported on this platform")
    }
}

// ── Cluster ───────────────────────────────────────────────────────────────────

/// One remote writer + two independent local nodes sharing the same Databend
/// backend, each with its own isolated `TempDir`.
pub struct Cluster {
    pub remote: Remote,
    pub local_a: Local,
    pub local_b: Local,
}

impl Cluster {
    pub async fn new(_user_id: Option<&str>) -> Result<Self> {
        let root_pool = pool().await?;
        let id = uid("s");
        let prefix = format!("ts{}_", id.replace('-', ""));
        let databases = Arc::new(AgentDatabases::new(root_pool.clone(), &prefix)?);

        // Create an agent database so the syncer can discover it.
        let agent_id = uid("agent");
        let agent_db = databases.agent_database_name(&agent_id);
        root_pool
            .exec(&format!("CREATE DATABASE IF NOT EXISTS `{agent_db}`"))
            .await?;
        let agent_pool = databases.agent_pool(&agent_id)?;
        // Run skill migrations on the agent database.
        let migration = include_str!("../../../../../migrations/0004_skills.sql");
        let stmts: Vec<&str> = migration
            .split(';')
            .map(|s: &str| s.trim())
            .filter(|s: &&str| !s.is_empty())
            .collect();
        for stmt in stmts {
            agent_pool.exec(stmt).await?;
        }

        let remote = Remote {
            store: DatabendSkillRepository::new(agent_pool),
        };
        let dir_a = TempDir::new()?;
        let dir_b = TempDir::new()?;
        let local_a = Local {
            store: SkillCatalogImpl::new(databases.clone(), dir_a.path().to_path_buf()),
            dir: dir_a,
        };
        let local_b = Local {
            store: SkillCatalogImpl::new(databases, dir_b.path().to_path_buf()),
            dir: dir_b,
        };
        Ok(Self {
            remote,
            local_a,
            local_b,
        })
    }
}
