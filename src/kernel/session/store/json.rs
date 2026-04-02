//! JsonSessionStore — per-session JSON file persistence for bendclaw-local.
//!
//! base_dir IS the session root. No session_id subdirectory.
//!
//! Layout:
//! ```text
//! {base_dir}/
//!   session.json
//!   runs/{run_id}.json
//!   events/{run_id}.jsonl
//!   usage/{run_id}.json
//! ```

use std::path::Path;
use std::path::PathBuf;

use async_trait::async_trait;

use super::contract::SessionStore;
use crate::kernel::run::usage::CostSummary;
use crate::kernel::run::usage::UsageEvent;
use crate::kernel::run::usage::UsageScope;
use crate::storage::dal::run::record::RunRecord;
use crate::storage::dal::run::record::RunStatus;
use crate::storage::dal::run_event::record::RunEventRecord;
use crate::storage::dal::session::record::SessionRecord;
use crate::storage::dal::session::repo::SessionWrite;
use crate::types::ErrorCode;
use crate::types::Result;

pub struct JsonSessionStore {
    base_dir: PathBuf,
}

impl JsonSessionStore {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    fn session_file(&self) -> PathBuf {
        self.base_dir.join("session.json")
    }

    fn run_file(&self, run_id: &str) -> PathBuf {
        self.base_dir.join("runs").join(format!("{run_id}.json"))
    }

    fn events_file(&self, run_id: &str) -> PathBuf {
        self.base_dir.join("events").join(format!("{run_id}.jsonl"))
    }

    fn usage_file(&self, run_id: &str) -> PathBuf {
        self.base_dir.join("usage").join(format!("{run_id}.json"))
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

impl JsonSessionStore {
    fn ensure_dir(path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ErrorCode::internal(format!("create dir: {e}")))?;
        }
        Ok(())
    }

    fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>> {
        match std::fs::read_to_string(path) {
            Ok(data) => {
                let val = serde_json::from_str(&data)
                    .map_err(|e| ErrorCode::internal(format!("parse {}: {e}", path.display())))?;
                Ok(Some(val))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(ErrorCode::internal(format!("read {}: {e}", path.display()))),
        }
    }

    fn write_json<T: serde::Serialize>(path: &Path, val: &T) -> Result<()> {
        Self::ensure_dir(path)?;
        let data = serde_json::to_string_pretty(val)
            .map_err(|e| ErrorCode::internal(format!("serialize: {e}")))?;
        std::fs::write(path, data)
            .map_err(|e| ErrorCode::internal(format!("write {}: {e}", path.display())))
    }

    fn append_jsonl<T: serde::Serialize>(path: &Path, val: &T) -> Result<()> {
        use std::io::Write;
        Self::ensure_dir(path)?;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| ErrorCode::internal(format!("open {}: {e}", path.display())))?;
        let line = serde_json::to_string(val)
            .map_err(|e| ErrorCode::internal(format!("serialize: {e}")))?;
        writeln!(file, "{line}")
            .map_err(|e| ErrorCode::internal(format!("write {}: {e}", path.display())))
    }
}

fn session_record_from_write(w: &SessionWrite) -> SessionRecord {
    let now = crate::storage::time::now().to_rfc3339();
    SessionRecord {
        id: w.session_id.clone(),
        agent_id: w.agent_id.clone(),
        user_id: w.user_id.clone(),
        title: w.title.clone(),
        scope: "private".to_string(),
        base_key: w.base_key.clone(),
        replaced_by_session_id: w.replaced_by_session_id.clone(),
        reset_reason: w.reset_reason.clone(),
        session_state: w.session_state.clone(),
        meta: w.meta.clone(),
        created_at: now.clone(),
        updated_at: now,
    }
}

// ── SessionStore impl ───────────────────────────────────────────────

#[async_trait]
impl SessionStore for JsonSessionStore {
    async fn session_load(&self, _id: &str) -> Result<Option<SessionRecord>> {
        Self::read_json(&self.session_file())
    }

    async fn session_upsert(&self, record: SessionWrite) -> Result<()> {
        let path = self.session_file();
        let rec = match Self::read_json::<SessionRecord>(&path)? {
            Some(mut existing) => {
                existing.title = record.title;
                existing.session_state = record.session_state;
                existing.meta = record.meta;
                existing.updated_at = crate::storage::time::now().to_rfc3339();
                existing
            }
            None => session_record_from_write(&record),
        };
        Self::write_json(&path, &rec)
    }

    async fn run_insert(&self, record: &RunRecord) -> Result<()> {
        let path = self.run_file(&record.id);
        Self::write_json(&path, record)
    }

    async fn run_update_final(
        &self,
        run_id: &str,
        status: RunStatus,
        output: &str,
        error: &str,
        metrics: &str,
        stop_reason: &str,
        iterations: u32,
    ) -> Result<()> {
        let path = self.run_file(run_id);
        let mut rec: RunRecord = Self::read_json(&path)?
            .ok_or_else(|| ErrorCode::internal(format!("run {run_id} not found")))?;
        rec.status = status.as_str().to_string();
        rec.output = output.to_string();
        rec.error = error.to_string();
        rec.metrics = metrics.to_string();
        rec.stop_reason = stop_reason.to_string();
        rec.iterations = iterations;
        rec.updated_at = crate::storage::time::now().to_rfc3339();
        Self::write_json(&path, &rec)
    }

    async fn run_update_status(&self, run_id: &str, status: RunStatus) -> Result<()> {
        let path = self.run_file(run_id);
        let mut rec: RunRecord = Self::read_json(&path)?
            .ok_or_else(|| ErrorCode::internal(format!("run {run_id} not found")))?;
        rec.status = status.as_str().to_string();
        rec.updated_at = crate::storage::time::now().to_rfc3339();
        Self::write_json(&path, &rec)
    }

    async fn run_list_by_session(&self, _session_id: &str, limit: u32) -> Result<Vec<RunRecord>> {
        let runs_dir = self.base_dir.join("runs");
        let mut records = Vec::new();
        let entries = match std::fs::read_dir(&runs_dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(records),
            Err(e) => return Err(ErrorCode::internal(format!("read runs dir: {e}"))),
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Some(rec) = Self::read_json::<RunRecord>(&path)? {
                    records.push(rec);
                }
            }
        }
        records.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        records.truncate(limit as usize);
        Ok(records)
    }

    async fn run_load_latest_checkpoint(&self, session_id: &str) -> Result<Option<RunRecord>> {
        let runs = self.run_list_by_session(session_id, u32::MAX).await?;
        Ok(runs.into_iter().rfind(|r| r.kind == "session_checkpoint"))
    }

    async fn run_events_insert_batch(&self, records: &[RunEventRecord]) -> Result<()> {
        for record in records {
            let path = self.events_file(&record.run_id);
            Self::append_jsonl(&path, record)?;
        }
        Ok(())
    }

    async fn usage_record(&self, event: UsageEvent) -> Result<()> {
        let path = self.usage_file(&event.run_id);
        Self::write_json(&path, &event)
    }

    async fn usage_flush(&self) -> Result<()> {
        Ok(())
    }

    async fn usage_summarize(&self, _scope: UsageScope) -> Result<CostSummary> {
        let mut summary = CostSummary::default();
        let usage_dir = self.base_dir.join("usage");
        let files = match std::fs::read_dir(&usage_dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(summary),
            Err(e) => return Err(ErrorCode::internal(format!("read usage dir: {e}"))),
        };
        for file_entry in files.flatten() {
            let path = file_entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Some(evt) = Self::read_json::<UsageEvent>(&path)? {
                    summary.total_prompt_tokens += evt.prompt_tokens;
                    summary.total_completion_tokens += evt.completion_tokens;
                    summary.total_reasoning_tokens += evt.reasoning_tokens;
                    summary.total_tokens += evt.prompt_tokens + evt.completion_tokens;
                    summary.total_cache_read_tokens += evt.cache_read_tokens;
                    summary.total_cache_write_tokens += evt.cache_write_tokens;
                    summary.total_cost += evt.cost;
                    summary.record_count += 1;
                }
            }
        }
        Ok(summary)
    }
}
