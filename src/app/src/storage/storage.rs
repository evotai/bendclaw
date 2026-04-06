use async_trait::async_trait;

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

#[async_trait]
pub trait Storage: Send + Sync {
    async fn put_session(&self, session: SessionMeta) -> Result<()>;
    async fn get_session(&self, session_id: &str) -> Result<Option<SessionMeta>>;
    async fn list_sessions(&self, params: ListSessions) -> Result<Vec<SessionMeta>>;

    async fn put_transcript_entries(&self, entries: Vec<TranscriptEntry>) -> Result<()>;
    async fn list_transcript_entries(
        &self,
        params: ListTranscriptEntries,
    ) -> Result<Vec<TranscriptEntry>>;

    async fn put_run(&self, run: RunMeta) -> Result<()>;
    async fn get_run(&self, run_id: &str) -> Result<Option<RunMeta>>;
    async fn list_runs(&self, params: ListRuns) -> Result<Vec<RunMeta>>;

    async fn put_run_events(&self, events: Vec<RunEvent>) -> Result<()>;
    async fn list_run_events(&self, params: ListRunEvents) -> Result<Vec<RunEvent>>;

    async fn put_trace(&self, trace: TraceMeta) -> Result<()>;
    async fn get_trace(&self, trace_id: &str) -> Result<Option<TraceMeta>>;
    async fn list_traces(&self, params: ListTraces) -> Result<Vec<TraceMeta>>;

    async fn put_trace_events(&self, events: Vec<TraceEvent>) -> Result<()>;
    async fn list_trace_events(&self, params: ListTraceEvents) -> Result<Vec<TraceEvent>>;
}
