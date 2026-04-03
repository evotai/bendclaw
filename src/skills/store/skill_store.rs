//! Shared skill store — IO layer.
//!
//! Pure CRUD over `evotai_meta.skills` + `evotai_meta.skill_files`.

use std::collections::HashMap;

use async_trait::async_trait;

use crate::skills::definition::skill::Skill;
use crate::skills::definition::skill::SkillFile;
use crate::skills::definition::skill::SkillId;
use crate::skills::definition::skill::SkillScope;
use crate::skills::definition::skill::SkillSource;
use crate::storage::pool::Pool;
use crate::storage::sql;
use crate::storage::sql::SqlVal;
use crate::types::Result;

const SKILLS_TABLE: &str = "evotai_meta.skills";
const FILES_TABLE: &str = "evotai_meta.skill_files";

// ── Trait ──

#[async_trait]
pub trait SharedSkillStore: Send + Sync + 'static {
    async fn list(&self, user_id: &str) -> Result<Vec<Skill>>;
    async fn get(&self, user_id: &str, name: &str) -> Result<Option<Skill>>;
    async fn save(&self, user_id: &str, skill: &Skill) -> Result<()>;
    async fn remove(&self, user_id: &str, name: &str) -> Result<()>;
    async fn checksums(&self, user_id: &str) -> Result<HashMap<String, String>>;
    async fn touch_last_used(&self, id: &SkillId, agent_id: &str) -> Result<()>;
    async fn list_shared(&self, user_id: &str) -> Result<Vec<Skill>>;
}

// ── Implementation ──

pub struct DatabendSharedSkillStore {
    pool: Pool,
}

impl DatabendSharedSkillStore {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }

    pub fn noop() -> Self {
        Self { pool: Pool::noop() }
    }
}

const SKILL_COLS: &str = "name, version, scope, source, user_id, created_by, \
                          description, timeout, executable, content";

fn parse_skill(row: &serde_json::Value) -> Result<Skill> {
    let exec_str: String = sql::col(row, 8);
    Ok(Skill {
        name: sql::col(row, 0),
        version: sql::col(row, 1),
        scope: SkillScope::parse(&sql::col(row, 2)),
        source: SkillSource::parse(&sql::col(row, 3)),
        user_id: sql::col(row, 4),
        created_by: sql::col_opt(row, 5),
        last_used_by: None,
        description: sql::col(row, 6),
        timeout: sql::col_u64(row, 7)?,
        executable: matches!(exec_str.as_str(), "1" | "true"),
        content: sql::col(row, 9),
        parameters: Vec::new(),
        files: Vec::new(),
        requires: None,
        manifest: None,
    })
}

fn parse_file(row: &serde_json::Value) -> Result<SkillFile> {
    Ok(SkillFile {
        path: sql::col(row, 0),
        body: sql::col(row, 1),
    })
}

#[async_trait]
impl SharedSkillStore for DatabendSharedSkillStore {
    async fn list(&self, user_id: &str) -> Result<Vec<Skill>> {
        let stmt = format!(
            "SELECT {SKILL_COLS} FROM {SKILLS_TABLE} \
             WHERE user_id = {} AND enabled = TRUE ORDER BY name ASC LIMIT 10000",
            SqlVal::Str(user_id).render(),
        );
        let rows = self.pool.query_all(&stmt).await?;
        rows.iter().map(parse_skill).collect()
    }

    async fn get(&self, user_id: &str, name: &str) -> Result<Option<Skill>> {
        let stmt = format!(
            "SELECT {SKILL_COLS} FROM {SKILLS_TABLE} \
             WHERE user_id = {} AND name = {} AND enabled = TRUE LIMIT 1",
            SqlVal::Str(user_id).render(),
            SqlVal::Str(name).render(),
        );
        let row = self.pool.query_row(&stmt).await?;
        let mut skill = match row.as_ref().map(parse_skill).transpose()? {
            Some(s) => s,
            None => return Ok(None),
        };

        let file_stmt = format!(
            "SELECT file_path, file_body FROM {FILES_TABLE} \
             WHERE skill_name = {} AND user_id = {} ORDER BY file_path ASC LIMIT 10000",
            SqlVal::Str(name).render(),
            SqlVal::Str(user_id).render(),
        );
        let file_rows = self.pool.query_all(&file_stmt).await?;
        skill.files = file_rows
            .iter()
            .filter_map(|r| parse_file(r).ok())
            .collect();

        Ok(Some(skill))
    }

    async fn save(&self, user_id: &str, skill: &Skill) -> Result<()> {
        self.remove(user_id, &skill.name).await?;

        let sha256 = skill.compute_sha256();
        let created_by = skill.created_by.as_deref().unwrap_or("");
        let stmt = format!(
            "INSERT INTO {SKILLS_TABLE} \
             (name, version, scope, source, user_id, created_by, description, timeout, \
              executable, enabled, content, sha256, updated_at) \
             VALUES ({}, {}, {}, {}, {}, {}, {}, {}, {}, TRUE, {}, {}, NOW())",
            SqlVal::Str(&skill.name).render(),
            SqlVal::Str(&skill.version).render(),
            SqlVal::Str(skill.scope.as_str()).render(),
            SqlVal::Str(skill.source.as_str()).render(),
            SqlVal::Str(user_id).render(),
            SqlVal::Str(created_by).render(),
            SqlVal::Str(&skill.description).render(),
            skill.timeout,
            if skill.executable { "TRUE" } else { "FALSE" },
            SqlVal::Str(&skill.content).render(),
            SqlVal::Str(&sha256).render(),
        );
        self.pool.exec(&stmt).await?;

        // Insert files + SKILL.md
        let mut file_stmts = Vec::new();
        for f in &skill.files {
            file_stmts.push(format!(
                "INSERT INTO {FILES_TABLE} \
                 (skill_name, user_id, created_by, file_path, file_body, sha256, updated_at) \
                 VALUES ({}, {}, {}, {}, {}, '', NOW())",
                SqlVal::Str(&skill.name).render(),
                SqlVal::Str(user_id).render(),
                SqlVal::Str(created_by).render(),
                SqlVal::Str(&f.path).render(),
                SqlVal::Str(&f.body).render(),
            ));
        }
        if !skill.content.is_empty() {
            file_stmts.push(format!(
                "INSERT INTO {FILES_TABLE} \
                 (skill_name, user_id, created_by, file_path, file_body, sha256, updated_at) \
                 VALUES ({}, {}, {}, 'SKILL.md', {}, '', NOW())",
                SqlVal::Str(&skill.name).render(),
                SqlVal::Str(user_id).render(),
                SqlVal::Str(created_by).render(),
                SqlVal::Str(&skill.content).render(),
            ));
        }
        for stmt in file_stmts {
            self.pool.exec(&stmt).await?;
        }

        Ok(())
    }

    async fn remove(&self, user_id: &str, name: &str) -> Result<()> {
        let del_files = format!(
            "DELETE FROM {FILES_TABLE} WHERE skill_name = {} AND user_id = {}",
            SqlVal::Str(name).render(),
            SqlVal::Str(user_id).render(),
        );
        let del_skill = format!(
            "DELETE FROM {SKILLS_TABLE} WHERE name = {} AND user_id = {}",
            SqlVal::Str(name).render(),
            SqlVal::Str(user_id).render(),
        );
        let _ = self.pool.exec(&del_files).await;
        self.pool.exec(&del_skill).await
    }

    async fn checksums(&self, user_id: &str) -> Result<HashMap<String, String>> {
        let stmt = format!(
            "SELECT name, sha256 FROM {SKILLS_TABLE} \
             WHERE user_id = {} AND enabled = TRUE ORDER BY name ASC LIMIT 10000",
            SqlVal::Str(user_id).render(),
        );
        let rows = self.pool.query_all(&stmt).await?;
        Ok(rows
            .iter()
            .map(|r| (sql::col(r, 0), sql::col(r, 1)))
            .collect())
    }

    async fn touch_last_used(&self, id: &SkillId, agent_id: &str) -> Result<()> {
        let stmt = format!(
            "UPDATE {SKILLS_TABLE} SET last_used_by = {} WHERE name = {} AND user_id = {}",
            SqlVal::Str(agent_id).render(),
            SqlVal::Str(&id.name).render(),
            SqlVal::Str(&id.owner_id).render(),
        );
        self.pool.exec(&stmt).await
    }

    async fn list_shared(&self, user_id: &str) -> Result<Vec<Skill>> {
        let stmt = format!(
            "SELECT {SKILL_COLS} FROM {SKILLS_TABLE} \
             WHERE scope = 'shared' AND user_id != {} AND enabled = TRUE \
             ORDER BY name ASC LIMIT 10000",
            SqlVal::Str(user_id).render(),
        );
        let rows = self.pool.query_all(&stmt).await?;
        rows.iter().map(parse_skill).collect()
    }
}
