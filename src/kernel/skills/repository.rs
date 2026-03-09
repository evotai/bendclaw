//! Skill persistence: repository trait + Databend implementation.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::base::Result;
use crate::kernel::skills::skill::Skill;
use crate::kernel::skills::skill::SkillFile;
use crate::kernel::skills::skill::SkillScope;
use crate::kernel::skills::skill::SkillSource;
use crate::storage::pool::Pool;
use crate::storage::sql;
use crate::storage::sql::SqlVal;
use crate::storage::table::DatabendTable;
use crate::storage::table::RowMapper;

// ── Repository trait ──────────────────────────────────────────────────────────

/// Async repository for skill persistence.
#[async_trait]
pub trait SkillRepository: Send + Sync + 'static {
    async fn list(&self) -> Result<Vec<Skill>>;
    async fn get(&self, name: &str) -> Result<Option<Skill>>;
    async fn save(&self, skill: &Skill) -> Result<()>;
    async fn remove(&self, name: &str, agent_id: Option<&str>, user_id: Option<&str>)
        -> Result<()>;
    /// `name → sha256` map for incremental sync diff.
    async fn checksums(&self) -> Result<HashMap<String, String>>;
}

/// Factory that produces agent-scoped [`SkillRepository`] instances.
pub trait SkillRepositoryFactory: Send + Sync + 'static {
    fn for_agent(&self, agent_id: &str) -> Result<Arc<dyn SkillRepository>>;
}

// ── Row mappers ───────────────────────────────────────────────────────────────

fn ownership_clause(agent_id: Option<&str>, user_id: Option<&str>) -> String {
    let agent_part = match agent_id {
        Some(id) => format!("agent_id = '{}'", sql::escape(id)),
        None => "agent_id IS NULL".to_string(),
    };
    let user_part = match user_id {
        Some(id) => format!("user_id = '{}'", sql::escape(id)),
        None => "user_id IS NULL".to_string(),
    };
    format!("{agent_part} AND {user_part}")
}

#[derive(Clone)]
struct SkillMapper;

impl RowMapper for SkillMapper {
    type Entity = Skill;

    fn columns(&self) -> &str {
        "name, version, scope, source, agent_id, user_id, description, timeout, executable, content"
    }

    fn parse(&self, row: &serde_json::Value) -> Skill {
        Skill {
            name: sql::col(row, 0),
            version: sql::col(row, 1),
            scope: SkillScope::parse(&sql::col(row, 2)),
            source: SkillSource::parse(&sql::col(row, 3)),
            agent_id: sql::col_opt(row, 4),
            user_id: sql::col_opt(row, 5),
            description: sql::col(row, 6),
            timeout: sql::col(row, 7).parse().unwrap_or(30),
            executable: matches!(sql::col(row, 8).as_str(), "1" | "true"),
            content: sql::col(row, 9),
            parameters: Vec::new(),
            files: Vec::new(),
            requires: None,
        }
    }
}

#[derive(Clone)]
struct FileMapper;

impl RowMapper for FileMapper {
    type Entity = SkillFile;

    fn columns(&self) -> &str {
        "file_path, file_body"
    }

    fn parse(&self, row: &serde_json::Value) -> SkillFile {
        SkillFile {
            path: sql::col(row, 0),
            body: sql::col(row, 1),
        }
    }
}

#[derive(Clone)]
struct ChecksumMapper;

impl RowMapper for ChecksumMapper {
    type Entity = (String, String);

    fn columns(&self) -> &str {
        "name, sha256"
    }

    fn parse(&self, row: &serde_json::Value) -> (String, String) {
        (sql::col(row, 0), sql::col(row, 1))
    }
}

// ── DatabendSkillRepository ───────────────────────────────────────────────────

/// Databend-backed skill repository scoped to a single agent's database.
pub struct DatabendSkillRepository {
    skills: DatabendTable<SkillMapper>,
    files: DatabendTable<FileMapper>,
    checksums_table: DatabendTable<ChecksumMapper>,
}

impl DatabendSkillRepository {
    pub fn new(pool: Pool) -> Self {
        Self {
            skills: DatabendTable::new(pool.clone(), "skills", SkillMapper),
            files: DatabendTable::new(pool.clone(), "skill_files", FileMapper),
            checksums_table: DatabendTable::new(pool, "skills", ChecksumMapper),
        }
    }
}

#[async_trait]
impl SkillRepository for DatabendSkillRepository {
    async fn list(&self) -> Result<Vec<Skill>> {
        self.skills
            .list_where("enabled = TRUE", "name ASC", 10000)
            .await
    }

    async fn get(&self, name: &str) -> Result<Option<Skill>> {
        let cond = format!("name = '{}' AND enabled = TRUE", sql::escape(name));
        let mut skill = match self.skills.get_where(&cond).await? {
            Some(s) => s,
            None => return Ok(None),
        };

        let file_cond = format!("skill_name = '{}'", sql::escape(name));
        skill.files = self
            .files
            .list_where(&file_cond, "file_path ASC", 10000)
            .await?;

        Ok(Some(skill))
    }

    async fn save(&self, skill: &Skill) -> Result<()> {
        self.remove(
            &skill.name,
            skill.agent_id.as_deref(),
            skill.user_id.as_deref(),
        )
        .await?;

        let sha256 = skill.compute_sha256();
        let agent_id_val = skill.agent_id.as_deref().map(sql::escape);
        let user_id_val = skill.user_id.as_deref().map(sql::escape);

        self.skills
            .insert(&[
                ("name", SqlVal::Str(&skill.name)),
                ("version", SqlVal::Str(&skill.version)),
                ("scope", SqlVal::Str(skill.scope.as_str())),
                ("source", SqlVal::Str(skill.source.as_str())),
                ("agent_id", match &agent_id_val {
                    Some(v) => SqlVal::Str(v),
                    None => SqlVal::Null,
                }),
                ("user_id", match &user_id_val {
                    Some(v) => SqlVal::Str(v),
                    None => SqlVal::Null,
                }),
                ("description", SqlVal::Str(&skill.description)),
                ("timeout", SqlVal::Int(skill.timeout as i64)),
                (
                    "executable",
                    SqlVal::Raw(if skill.executable { "TRUE" } else { "FALSE" }),
                ),
                ("enabled", SqlVal::Raw("TRUE")),
                ("content", SqlVal::Str(&skill.content)),
                ("sha256", SqlVal::Str(&sha256)),
                ("updated_at", SqlVal::Raw("NOW()")),
            ])
            .await?;

        if !skill.files.is_empty() {
            let columns = &[
                "skill_name",
                "agent_id",
                "user_id",
                "file_path",
                "file_body",
                "sha256",
                "updated_at",
            ];
            let rows: Vec<Vec<SqlVal<'_>>> = skill
                .files
                .iter()
                .map(|f| {
                    vec![
                        SqlVal::Str(&skill.name),
                        match &agent_id_val {
                            Some(v) => SqlVal::Str(v),
                            None => SqlVal::Null,
                        },
                        match &user_id_val {
                            Some(v) => SqlVal::Str(v),
                            None => SqlVal::Null,
                        },
                        SqlVal::Str(&f.path),
                        SqlVal::Str(&f.body),
                        SqlVal::Str(""),
                        SqlVal::Raw("NOW()"),
                    ]
                })
                .collect();
            self.files.insert_batch(columns, &rows).await?;
        }

        tracing::info!(
            skill = %skill.name,
            version = %skill.version,
            scope = %skill.scope,
            source = %skill.source,
            files = skill.files.len(),
            "skill saved"
        );

        Ok(())
    }

    async fn remove(
        &self,
        name: &str,
        agent_id: Option<&str>,
        user_id: Option<&str>,
    ) -> Result<()> {
        let ownership = ownership_clause(agent_id, user_id);

        let file_cond = format!("skill_name = '{}' AND {}", sql::escape(name), ownership);
        self.files.delete_where(&file_cond).await?;

        let skill_cond = format!("name = '{}' AND {}", sql::escape(name), ownership);
        self.skills.delete_where(&skill_cond).await?;

        tracing::info!(skill = %name, ?agent_id, ?user_id, "skill removed");
        Ok(())
    }

    async fn checksums(&self) -> Result<HashMap<String, String>> {
        let rows = self
            .checksums_table
            .list_where("enabled = TRUE", "name ASC", 10000)
            .await?;
        Ok(rows.into_iter().collect())
    }
}

// ── Factory ───────────────────────────────────────────────────────────────────

/// Factory that creates agent-scoped [`DatabendSkillRepository`] instances.
pub struct DatabendSkillRepositoryFactory {
    databases: Arc<crate::storage::AgentDatabases>,
}

impl DatabendSkillRepositoryFactory {
    pub fn new(databases: Arc<crate::storage::AgentDatabases>) -> Self {
        Self { databases }
    }
}

impl SkillRepositoryFactory for DatabendSkillRepositoryFactory {
    fn for_agent(&self, agent_id: &str) -> Result<Arc<dyn SkillRepository>> {
        let pool = self.databases.agent_pool(agent_id)?;
        Ok(Arc::new(DatabendSkillRepository::new(pool)))
    }
}
