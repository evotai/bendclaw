use std::path::Path;
use std::path::PathBuf;

use async_trait::async_trait;
use tokio::fs;

use crate::error::BendclawError;
use crate::error::Result;
use crate::protocol::ListRunEvents;
use crate::protocol::ListRuns;
use crate::protocol::ListSessions;
use crate::protocol::ListTraceEvents;
use crate::protocol::ListTraces;
use crate::protocol::ListTranscriptEntries;
use crate::protocol::RunEvent;
use crate::protocol::RunMeta;
use crate::protocol::SessionMeta;
use crate::protocol::TraceEvent;
use crate::protocol::TraceMeta;
use crate::protocol::TranscriptEntry;
use crate::storage::Storage;

pub struct FsStorage {
    root_dir: PathBuf,
}

impl FsStorage {
    pub fn new(root_dir: PathBuf) -> Self {
        Self { root_dir }
    }

    fn sessions_dir(&self) -> PathBuf {
        self.root_dir.join("sessions")
    }

    fn session_dir(&self, session_id: &str) -> PathBuf {
        self.sessions_dir().join(session_id)
    }

    fn session_meta_path(&self, session_id: &str) -> PathBuf {
        self.session_dir(session_id).join("session.json")
    }

    fn transcript_path(&self, session_id: &str) -> PathBuf {
        self.session_dir(session_id).join("transcript.jsonl")
    }

    fn run_dir(&self, session_id: &str) -> PathBuf {
        self.session_dir(session_id).join("runs")
    }

    fn run_meta_path(&self, session_id: &str, run_id: &str) -> PathBuf {
        self.run_dir(session_id).join(format!("{run_id}.json"))
    }

    fn run_events_path(&self, session_id: &str, run_id: &str) -> PathBuf {
        self.run_dir(session_id).join(format!("{run_id}.jsonl"))
    }

    fn trace_dir(&self, session_id: &str) -> PathBuf {
        self.session_dir(session_id).join("traces")
    }

    fn trace_meta_path(&self, session_id: &str, trace_id: &str) -> PathBuf {
        self.trace_dir(session_id).join(format!("{trace_id}.json"))
    }

    fn trace_events_path(&self, session_id: &str, trace_id: &str) -> PathBuf {
        self.trace_dir(session_id).join(format!("{trace_id}.jsonl"))
    }

    async fn write_json<T: serde::Serialize>(&self, path: PathBuf, value: &T) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let json = serde_json::to_string_pretty(value)?;
        fs::write(path, json).await?;
        Ok(())
    }

    async fn read_json<T: serde::de::DeserializeOwned>(&self, path: &Path) -> Result<Option<T>> {
        match fs::read_to_string(path).await {
            Ok(content) => Ok(Some(serde_json::from_str(&content)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(BendclawError::Io(e)),
        }
    }

    async fn write_jsonl<T: serde::Serialize>(&self, path: PathBuf, values: &[T]) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let mut lines = String::new();
        for value in values {
            lines.push_str(&serde_json::to_string(value)?);
            lines.push('\n');
        }
        fs::write(path, lines).await?;
        Ok(())
    }

    async fn read_jsonl<T: serde::de::DeserializeOwned>(&self, path: &Path) -> Result<Vec<T>> {
        match fs::read_to_string(path).await {
            Ok(content) => {
                let mut values = Vec::new();
                for line in content.lines() {
                    if !line.trim().is_empty() {
                        values.push(serde_json::from_str(line)?);
                    }
                }
                Ok(values)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(BendclawError::Io(e)),
        }
    }

    async fn find_run_meta_path(&self, run_id: &str) -> Result<Option<PathBuf>> {
        let mut sessions = match fs::read_dir(self.sessions_dir()).await {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(BendclawError::Io(e)),
        };

        while let Some(entry) = sessions.next_entry().await? {
            let path = entry.path().join("runs").join(format!("{run_id}.json"));
            if fs::try_exists(&path).await? {
                return Ok(Some(path));
            }
        }

        Ok(None)
    }

    async fn find_run_events_path(&self, run_id: &str) -> Result<Option<PathBuf>> {
        let mut sessions = match fs::read_dir(self.sessions_dir()).await {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(BendclawError::Io(e)),
        };

        while let Some(entry) = sessions.next_entry().await? {
            let path = entry.path().join("runs").join(format!("{run_id}.jsonl"));
            if fs::try_exists(&path).await? {
                return Ok(Some(path));
            }
        }

        Ok(None)
    }

    async fn find_trace_meta_path(&self, trace_id: &str) -> Result<Option<PathBuf>> {
        let mut sessions = match fs::read_dir(self.sessions_dir()).await {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(BendclawError::Io(e)),
        };

        while let Some(entry) = sessions.next_entry().await? {
            let path = entry.path().join("traces").join(format!("{trace_id}.json"));
            if fs::try_exists(&path).await? {
                return Ok(Some(path));
            }
        }

        Ok(None)
    }

    async fn find_trace_events_path(&self, trace_id: &str) -> Result<Option<PathBuf>> {
        let mut sessions = match fs::read_dir(self.sessions_dir()).await {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(BendclawError::Io(e)),
        };

        while let Some(entry) = sessions.next_entry().await? {
            let path = entry
                .path()
                .join("traces")
                .join(format!("{trace_id}.jsonl"));
            if fs::try_exists(&path).await? {
                return Ok(Some(path));
            }
        }

        Ok(None)
    }
}

#[async_trait]
impl Storage for FsStorage {
    async fn put_session(&self, session: SessionMeta) -> Result<()> {
        self.write_json(self.session_meta_path(&session.session_id), &session)
            .await
    }

    async fn get_session(&self, session_id: &str) -> Result<Option<SessionMeta>> {
        self.read_json(&self.session_meta_path(session_id)).await
    }

    async fn list_sessions(&self, params: ListSessions) -> Result<Vec<SessionMeta>> {
        let mut entries = match fs::read_dir(self.sessions_dir()).await {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(BendclawError::Io(e)),
        };

        let mut sessions = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path().join("session.json");
            if let Some(session) = self.read_json::<SessionMeta>(&path).await? {
                sessions.push(session);
            }
        }

        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        if params.limit > 0 {
            sessions.truncate(params.limit);
        }
        Ok(sessions)
    }

    async fn put_transcript_entries(&self, entries: Vec<TranscriptEntry>) -> Result<()> {
        let Some(first) = entries.first() else {
            return Ok(());
        };
        self.write_jsonl(self.transcript_path(&first.session_id), &entries)
            .await
    }

    async fn list_transcript_entries(
        &self,
        params: ListTranscriptEntries,
    ) -> Result<Vec<TranscriptEntry>> {
        let mut entries = self
            .read_jsonl::<TranscriptEntry>(&self.transcript_path(&params.session_id))
            .await?;

        if let Some(run_id) = &params.run_id {
            entries.retain(|entry| entry.run_id.as_ref() == Some(run_id));
        }
        if let Some(after_seq) = params.after_seq {
            entries.retain(|entry| entry.seq > after_seq);
        }
        if let Some(limit) = params.limit {
            entries.truncate(limit);
        }

        Ok(entries)
    }

    async fn put_run(&self, run: RunMeta) -> Result<()> {
        self.write_json(self.run_meta_path(&run.session_id, &run.run_id), &run)
            .await
    }

    async fn get_run(&self, run_id: &str) -> Result<Option<RunMeta>> {
        let Some(path) = self.find_run_meta_path(run_id).await? else {
            return Ok(None);
        };
        self.read_json(&path).await
    }

    async fn list_runs(&self, params: ListRuns) -> Result<Vec<RunMeta>> {
        let mut runs = Vec::new();

        let session_ids = if let Some(session_id) = params.session_id {
            vec![session_id]
        } else {
            self.list_sessions(ListSessions { limit: 0 })
                .await?
                .into_iter()
                .map(|session| session.session_id)
                .collect()
        };

        for session_id in session_ids {
            let dir = self.run_dir(&session_id);
            let mut entries = match fs::read_dir(&dir).await {
                Ok(entries) => entries,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(BendclawError::Io(e)),
            };

            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                    continue;
                }
                if let Some(run) = self.read_json::<RunMeta>(&path).await? {
                    runs.push(run);
                }
            }
        }

        runs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        if params.limit > 0 {
            runs.truncate(params.limit);
        }
        Ok(runs)
    }

    async fn put_run_events(&self, events: Vec<RunEvent>) -> Result<()> {
        let Some(first) = events.first() else {
            return Ok(());
        };
        self.write_jsonl(
            self.run_events_path(&first.session_id, &first.run_id),
            &events,
        )
        .await
    }

    async fn list_run_events(&self, params: ListRunEvents) -> Result<Vec<RunEvent>> {
        let Some(path) = self.find_run_events_path(&params.run_id).await? else {
            return Ok(Vec::new());
        };
        self.read_jsonl(&path).await
    }

    async fn put_trace(&self, trace: TraceMeta) -> Result<()> {
        self.write_json(
            self.trace_meta_path(&trace.session_id, &trace.trace_id),
            &trace,
        )
        .await
    }

    async fn get_trace(&self, trace_id: &str) -> Result<Option<TraceMeta>> {
        let Some(path) = self.find_trace_meta_path(trace_id).await? else {
            return Ok(None);
        };
        self.read_json(&path).await
    }

    async fn list_traces(&self, params: ListTraces) -> Result<Vec<TraceMeta>> {
        let mut traces = Vec::new();

        let session_ids = if let Some(session_id) = params.session_id {
            vec![session_id]
        } else {
            self.list_sessions(ListSessions { limit: 0 })
                .await?
                .into_iter()
                .map(|session| session.session_id)
                .collect()
        };

        for session_id in session_ids {
            let dir = self.trace_dir(&session_id);
            let mut entries = match fs::read_dir(&dir).await {
                Ok(entries) => entries,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(BendclawError::Io(e)),
            };

            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                    continue;
                }
                if let Some(trace) = self.read_json::<TraceMeta>(&path).await? {
                    if let Some(run_id) = &params.run_id {
                        if &trace.run_id != run_id {
                            continue;
                        }
                    }
                    traces.push(trace);
                }
            }
        }

        traces.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        if params.limit > 0 {
            traces.truncate(params.limit);
        }
        Ok(traces)
    }

    async fn put_trace_events(&self, events: Vec<TraceEvent>) -> Result<()> {
        let Some(first) = events.first() else {
            return Ok(());
        };
        self.write_jsonl(
            self.trace_events_path(&first.session_id, &first.trace_id),
            &events,
        )
        .await
    }

    async fn list_trace_events(&self, params: ListTraceEvents) -> Result<Vec<TraceEvent>> {
        let Some(path) = self.find_trace_events_path(&params.trace_id).await? else {
            return Ok(Vec::new());
        };
        self.read_jsonl(&path).await
    }
}
